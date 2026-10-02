import http from "node:http";
import https from "node:https";
import { createHash } from "node:crypto";
import { pipeline } from "node:stream";
import {
  TIERS,
  tierOf,
  idOf,
  availableTiers,
  tierSpec,
  isAuto,
  shouldUseExactModel,
} from "./config.mjs";
import { askJev } from "./router.mjs";
import { decide } from "./policy.mjs";
import { debug } from "./log.mjs";
import { writeDecision, markManual, dumpBody } from "./status.mjs";

const ANTHROPIC_BASE_URL = "https://api.anthropic.com";
const TYPESAFE_BASE_URL = "https://api.typesafe.ai";

// Hop-by-hop headers describe the client's connection to the proxy, not the proxy's to the API.
const HOP_BY_HOP = ["content-length", "transfer-encoding", "connection", "keep-alive"];

/**
 * Headers for the upstream request. The body may have been rewritten, so its length is set from
 * what is actually sent rather than dropped, which would make every request chunked.
 */
export function upstreamHeaders(incoming, host, body) {
  const headers = { ...incoming, host };
  for (const name of HOP_BY_HOP) delete headers[name];
  if (body.length) headers["content-length"] = String(body.length);
  return headers;
}

/**
 * Starts the TLS handshake with Jev before the first prompt needs it. Measured at ~0.9s on a
 * cold first call, which otherwise lands inside the user's first turn. The SDK uses the global
 * fetch, whose connection pool this warms; a failure here costs nothing.
 */
export function prewarmJev(base = process.env.TYPESAFE_BASE_URL ?? TYPESAFE_BASE_URL) {
  try {
    fetch(new URL(base).origin, { method: "HEAD" }).catch(() => {});
  } catch {
    // A malformed base URL fails properly on the first real call instead.
  }
}

/**
 * Claude Code converts draft-04 relics in MCP tool schemas before sending them first-party,
 * but skips that when ANTHROPIC_BASE_URL is set, so the API rejects the request. In draft
 * 2020-12 `exclusiveMinimum`/`exclusiveMaximum` are numbers, not booleans.
 */
export function sanitizeSchema(node) {
  if (Array.isArray(node)) return node.forEach(sanitizeSchema);
  if (!node || typeof node !== "object") return;
  for (const [key, bound] of [
    ["exclusiveMinimum", "minimum"],
    ["exclusiveMaximum", "maximum"],
  ]) {
    if (typeof node[key] === "boolean") {
      if (node[key] && typeof node[bound] === "number") {
        node[key] = node[bound];
        delete node[bound];
      } else {
        delete node[key];
      }
    }
  }
  for (const v of Object.values(node)) sanitizeSchema(v);
}

/**
 * The text of a genuinely new user turn, or null.
 *
 * A turn can continue for many requests while Claude works through tool calls, and those
 * continuations end in a `tool_result` rather than typed text. Routing them would re-ask
 * Jev on every tool call and let the model flip mid-task, so only the opening request of a
 * turn counts. Claude Code also injects `<system-reminder>` blocks into the user message,
 * which are noise to a router and measurably blunt Jev's confidence, so they are removed.
 */
export function newTurnPrompt(body) {
  if (!Array.isArray(body?.tools) || body.tools.length === 0) return null; // auxiliary call
  // Hook output, such as a SessionStart hook's context, arrives as a `system` message after the
  // user's prompt. It is not a turn of its own, and treating it as the last message meant the
  // first prompt of every session with a hook installed was never routed.
  const last = [...(body?.messages ?? [])].reverse().find((m) => m?.role !== "system");
  if (!last || last.role !== "user") return null;
  let text;
  if (typeof last.content === "string") {
    text = last.content;
  } else if (Array.isArray(last.content)) {
    if (last.content.some((b) => b.type === "tool_result")) return null;
    text = last.content
      .filter((b) => b.type === "text")
      .map((b) => b.text)
      .join("\n");
  } else {
    return null;
  }
  return text.replace(/<system-reminder>[\s\S]*?<\/system-reminder>/g, "").trim() || null;
}

/**
 * Points a request at a tier, removing request fields that tier cannot accept. Claude Code
 * composes the body for whatever model it thinks it is talking to, so downgrading to Haiku
 * while leaving `thinking: {type:"adaptive"}` in place is a hard 400.
 */
export function applyTier(body, tierName, model = idOf(tierName)) {
  const tier = tierSpec(tierName);
  if (!tier) return body;
  body.model = model;
  if (!tier.thinking) {
    delete body.thinking;
    // A context-management strategy that prunes thinking blocks is itself rejected once
    // thinking is gone, so it has to go with it.
    const edits = body.context_management?.edits;
    if (Array.isArray(edits)) {
      body.context_management.edits = edits.filter((e) => !/thinking/i.test(e?.type ?? ""));
      if (body.context_management.edits.length === 0) delete body.context_management;
    }
  }
  if (!tier.effort && body.output_config) {
    delete body.output_config.effort;
    if (Object.keys(body.output_config).length === 0) delete body.output_config;
  } else if (tier.effort && tier.floor && !body.output_config?.effort) {
    // Nothing asked for an effort, so the model's own default would apply - and those differ
    // between tiers, which would make a routing decision quietly change reasoning depth too.
    body.output_config = { ...body.output_config, effort: tier.floor };
  }
  return body;
}

/**
 * Exact Claude models reported by the account, newest first; static ids are the cold-start
 * fallback.
 *
 * The order is established here rather than assumed of the catalog: `/v1/models` lists every
 * version an account can reach (`claude-opus-4-5` through `claude-opus-5-5`), and whoever
 * takes the first entry of a tier gets whichever one the API happened to list first. Entries
 * without a release date keep their catalog order, which is the best guess left.
 */
export function claudeModels(catalog = []) {
  const models = catalog
    .filter((model) => tierOf(model?.id))
    .map((model) => ({
      id: model.id,
      tier: tierOf(model.id),
      releasedAt: model.created_at ?? "",
      description: [
        model.display_name,
        model.created_at && `released ${model.created_at.slice(0, 10)}`,
        model.max_input_tokens && `${model.max_input_tokens} input tokens`,
      ].filter(Boolean).join("; "),
    }))
    .sort((a, b) => b.releasedAt.localeCompare(a.releasedAt));
  return models.length
    ? models
    : TIERS.map((tier) => ({ id: tier.id, tier: tier.name, releasedAt: "", description: tier.id }));
}

/**
 * One model per tier: the newest of each.
 *
 * Jev is asked to pick an exact model, so every older version left in the list is a version
 * it can pick — and `claude-opus-5` answers the question "which opus" just as well as
 * `claude-opus-5-5` does. Offering only the newest of each tier makes the routing decision
 * about capability, which is what Jev is good at, and never about version.
 */
export function newestPerTier(models) {
  const newest = new Map();
  for (const model of models) if (!newest.has(model.tier)) newest.set(model.tier, model);
  return [...newest.values()];
}

const modelForTier = (models, tier) => models.find((model) => model.tier === tier)?.id ?? idOf(tier);

/**
 * Identifies the conversation a request belongs to. Claude Code runs sub-agents through the
 * same endpoint, so a single pinned model would let a sub-agent's choice leak into the main
 * conversation.
 *
 * Only stable fields may be used. Claude Code moves its `cache_control` breakpoint between
 * requests and rewrites message metadata, so the key is built from the session id plus the
 * text of the first message, which is fixed once a conversation starts and differs between
 * the main agent and each sub-agent.
 */
/**
 * Session id Claude Code embeds in request metadata, or "" when it isn't present.
 * `metadata.user_id` is a JSON string, not a plain id.
 */
export function sessionOf(body) {
  try {
    return JSON.parse(body?.metadata?.user_id ?? "{}").session_id ?? "";
  } catch {
    return "";
  }
}

export function conversationKey(body) {
  const session = sessionOf(body);
  const content = body?.messages?.[0]?.content;
  const text =
    typeof content === "string"
      ? content
      : Array.isArray(content)
        ? content
            .filter((b) => b.type === "text")
            .map((b) => b.text)
            .join("")
        : "";
  return createHash("sha1").update(`${session}|${text}`).digest("hex").slice(0, 12);
}

/**
 * A short human-readable name for a conversation, taken from its first message.
 *
 * `conversationKey` is a hash, which is useless in a status line. For a sub-agent the first
 * message is the task it was given, which is exactly what identifies it to the user; for the
 * main thread it is the opening prompt. System reminders are stripped because Claude Code
 * injects them into the first user message and they would crowd out the real text.
 */
export function agentLabel(body, max = 48) {
  const content = body?.messages?.[0]?.content;
  const text =
    typeof content === "string"
      ? content
      : Array.isArray(content)
        ? content.filter((b) => b.type === "text").map((b) => b.text).join(" ")
        : "";
  const clean = text
    .replace(/<system-reminder>[\s\S]*?<\/system-reminder>/g, "")
    .replace(/\s+/g, " ")
    .trim();
  return clean.length > max ? `${clean.slice(0, max - 1)}…` : clean;
}

/**
 * Which agent inside a session a request belongs to, and whether it is the main thread.
 *
 * Claude Code gives the main conversation and every sub-agent the same session id, so the
 * only thing separating them is `conversationKey`. The main thread is the first one to make
 * a real agent request in the session; `mains` remembers that so later sub-agents are not
 * mistaken for it. Requests without tools are ignored for this purpose because Claude Code's
 * own title and summary calls carry no tools and would otherwise claim the main slot.
 */
export function agentOf(body, mains) {
  const key = conversationKey(body);
  const session = sessionOf(body);
  const real = Array.isArray(body?.tools) && body.tools.length > 0;
  if (session && real && !mains.has(session)) {
    if (mains.size > 50) mains.delete(mains.keys().next().value);
    mains.set(session, key);
  }
  // With no session id there is nothing to compare against, so the conversation stands alone.
  const main = !session || mains.get(session) === key;
  return { key, label: agentLabel(body) || (main ? "main" : key), main };
}

// Conversations whose routing state is kept per proxy.
const MAX_CONVERSATIONS = 50;

export async function startProxy({ upstreamURL = ANTHROPIC_BASE_URL, route = askJev } = {}) {
  // Tier routed for each conversation's turn in flight, reused by its follow-up requests and
  // by the cache-rebuild guard, which needs to know what the prompt cache was built on.
  const convos = new Map();
  const catalog = new Map();
  // First conversation key seen per session: the main thread, as opposed to its sub-agents.
  const mains = new Map();
  /**
   * The routing state for a conversation, kept least-recently-used first, and never evicting a
   * main thread. The main thread is idle while its sub-agents run, so by recency alone it is
   * exactly the entry a session with 50 sub-agents throws out, and its next tool continuation
   * then jumped to the default tier mid-turn.
   *
   * `fallback` is the key the conversation had without a session id: some `claude -p` versions
   * omit the metadata on the first request, and a later request that carries it must find the
   * same state.
   */
  const stateFor = (key, fallback = null) => {
    let s = convos.get(key);
    if (!s && fallback && convos.has(fallback)) {
      s = convos.get(fallback);
      convos.delete(fallback);
    }
    if (s) convos.delete(key);
    else if (convos.size >= MAX_CONVERSATIONS) {
      const mainKeys = new Set(mains.values());
      const oldest = [...convos.keys()];
      convos.delete(oldest.find((k) => !mainKeys.has(k)) ?? oldest[0]);
    }
    convos.set(key, (s ??= { tier: null }));
    return s;
  };

  const server = http.createServer((req, res) => {
    // Claude Code probes the base URL before its first request.
    if (req.method === "HEAD") return res.writeHead(200).end();

    // Claude Code drops a request when the user presses Esc. Unless that reaches the API, it
    // keeps generating, and billing, the rest of a response nobody will read.
    let upstream = null;
    res.on("close", () => {
      if (!res.writableFinished) upstream?.destroy();
    });

    const chunks = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", async () => {
      let out = Buffer.concat(chunks);

      if (/^\/v1\/messages/.test(req.url ?? "")) {
        let body;
        let state = null;
        try {
          body = JSON.parse(out.toString());
          // Claude Code's request shape is undocumented and moves; JEV_DUMP captures it.
          dumpBody(body);
          body.tools?.forEach((t) => sanitizeSchema(t.input_schema));

          // Anything that is not the sentinel is a model the user chose, and an explicit
          // choice beats the router. That also covers Claude Code's own cheap Haiku calls
          // for titles and summaries, which must never be pinned up to the session's tier.
          if (!isAuto(body.model)) {
            debug(`passthrough, user selected ${body.model}`);
            // Only a real agent turn reflects the user's choice. Claude Code's own auxiliary
            // calls carry no tools and must not flip the status line to manual mid-session,
            // nor appear as an agent of their own.
            // Recorded once per turn: the choice cannot change between a turn's tool calls, and
            // each record is a read and rewrite of the session's status file.
            if (body.tools?.length) {
              const agent = agentOf(body, mains);
              debug(`${agent.key} passthrough ${agent.main ? "main" : "sub"} ${body.model}`);
              if (newTurnPrompt(body)) markManual(sessionOf(body), body.model, agent);
            }
          } else {
            const agent = agentOf(body, mains);
            const key = agent.key;
            state = stateFor(key, sessionOf(body) ? conversationKey({ ...body, metadata: undefined }) : null);
            // What the prompt cache was built on, which is what a downgrade would discard.
            const current = state.tier ?? "opus";
            const prompt = newTurnPrompt(body);
            const explaining = prompt?.includes("<jev-explain>");
            let fresh = null;
            if (prompt && !explaining) {
              const models = newestPerTier(
                claudeModels([...catalog.values()]).filter((model) =>
                  availableTiers().includes(model.tier),
                ),
              );
              const available = [...new Set(models.map((model) => model.tier))];
              const currentModel = state.model ?? modelForTier(models, current);
              const contextTokens = Math.round(JSON.stringify(body.messages).length / 4);
              const jev = await route({ prompt, current: currentModel, contextTokens, models });
              const chosen = models.find((model) => model.id === jev?.choice);
              // An answer naming a model that was not on the menu - an older version Jev knows
              // of, say - is treated as no answer, so policy decides. Carrying it forward left
              // the tier undefined and the sentinel model in the body, which the API rejects.
              const tierAnswer = chosen ? { ...jev, choice: chosen.tier } : null;
              const { tier, reason } = decide({
                prompt,
                jev: tierAnswer,
                current,
                available,
                // A conversation this proxy has not routed yet - a resumed session, a sub-agent
                // handed a large brief - has nothing cached on any model here, so there is no
                // cache for a downgrade to throw away.
                contextTokens: state.tier ? contextTokens : 0,
              });
              const model =
                shouldUseExactModel(reason, chosen?.tier, tier)
                  ? chosen.id
                  : tier === current
                    ? currentModel
                    : modelForTier(models, tier);
              state.tier = tier;
              state.model = model;
              fresh = {
                prompt,
                model,
                confidence: jev?.confidence ?? null,
                metrics: jev?.metrics ?? null,
                reason,
                jev: jev ? { request: jev.request, response: jev.response } : null,
              };
              debug(
                `${key} ${agent.main ? "main" : `sub[${agent.label}]`} ` +
                  `${jev ? `${jev.ms}ms p=${Number(jev.confidence).toFixed(2)}` : "no-jev"} ` +
                  `${current} -> ${tier} (${reason}) ctx~${contextTokens} | ${prompt.slice(0, 60)}`,
              );
            }
            // The sentinel is not a real model, so every routed request must be rewritten,
            // including follow-ups that reuse the tier chosen for the turn.
            const tier = state.tier ?? current;
            const model = state.model ?? idOf(tier);
            debug(`${key} rewrite ${body.model} -> ${model}`);
            applyTier(body, tier, model);
            // Publish what went out. Claude Code's UI shows the row you picked, not the tier
            // it resolved to, so the status line is the only place this is visible.
            // `claude -p` omits metadata on the first request of a session, so there is no
            // session id to file the decision under and it would be dropped. The conversation
            // key is stable for the same conversation and is already what `debug` prints, so
            // it is the identifier a user can pass to `jev-explain` for a print-mode run.
            if (fresh && !explaining) {
              writeDecision(sessionOf(body) || key, { tier, ...fresh, at: Date.now() }, agent);
            }
          }
          out = Buffer.from(JSON.stringify(body));
        } catch (err) {
          debug(`could not process body: ${err.message}`);
          // Whatever failed, the sentinel is not a model the API knows, and forwarding it is a
          // certain 400 for the user's turn. The conversation's tier, or the safe default, is not.
          if (isAuto(body?.model)) {
            const tier = state?.tier ?? "opus";
            applyTier(body, tier, state?.model ?? idOf(tier));
            out = Buffer.from(JSON.stringify(body));
          }
        }
      }

      // The user gave up while Jev was being asked; there is nobody to send this turn for.
      if (res.destroyed) return;

      const target = new URL(upstreamURL);
      const transport = target.protocol === "http:" ? http : https;
      const headers = upstreamHeaders(req.headers, target.host, out);
      if (req.method === "GET" && /^\/v1\/models(?:\?|$)/.test(req.url ?? "")) {
        delete headers["accept-encoding"];
      }
      // Under JEV_DEBUG, ask for an uncompressed stream so the model the API reports can be
      // read back out of it. Not worth the bandwidth cost in normal operation.
      if (process.env.JEV_DEBUG) delete headers["accept-encoding"];
      upstream = transport.request(
        {
          hostname: target.hostname,
          port: target.port || undefined,
          path: `${target.pathname.replace(/\/$/, "")}${req.url}`,
          method: req.method,
          headers,
        },
        (up) => {
          const isModels = req.method === "GET" && /^\/v1\/models(?:\?|$)/.test(req.url ?? "");
          if (isModels) {
            const chunks = [];
            up.on("error", (e) => res.destroy(e));
            up.on("data", (chunk) => chunks.push(chunk));
            up.on("end", () => {
              const data = Buffer.concat(chunks);
              try {
                for (const model of JSON.parse(data.toString()).data ?? []) {
                  if (tierOf(model?.id)) catalog.set(model.id, model);
                }
              } catch (err) {
                debug(`could not read Claude model catalog: ${err.message}`);
              }
              const headers = { ...up.headers };
              delete headers["content-length"];
              res.writeHead(up.statusCode, headers);
              res.end(data);
            });
            return;
          }
          res.writeHead(up.statusCode, up.headers);
          // Report the model the API itself says it used, so the routing can be confirmed
          // from the wire rather than trusted from our own decision log. Claude Code's UI
          // always shows the model it asked for, never the one we rewrote to.
          if (process.env.JEV_DEBUG) {
            let seen = false;
            up.on("data", (c) => {
              if (seen) return;
              const m = /"model"\s*:\s*"([^"]+)"/.exec(c.toString("utf8"));
              if (!m) return;
              seen = true;
              debug(`${up.statusCode} served by ${m[1]}`);
            });
          }
          // Ties the two streams together both ways: a client that leaves stops the upstream
          // read, and an upstream that drops mid-stream fails the client fast rather than
          // leaving Claude Code waiting on a response that will never finish.
          pipeline(up, res, (err) => {
            if (err) debug(`stream ended early: ${err.message}`);
          });
        },
      );
      upstream.on("error", (e) => {
        // Destroyed on purpose after the client left; there is no one to tell.
        if (res.destroyed) return;
        debug(`upstream error: ${e.message}`);
        // Mid-stream, an error body would be appended to an event stream as garbage.
        if (res.headersSent) return void res.destroy(e);
        res.writeHead(502, { "content-type": "application/json" });
        res.end(JSON.stringify({ type: "error", error: { message: e.message } }));
      });
      if (out.length) upstream.write(out);
      upstream.end();
    });
  });

  if (route === askJev) prewarmJev();
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return { port: server.address().port, close: () => server.close() };
}
