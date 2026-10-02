import http from "node:http";
import https from "node:https";
import { createHash, randomUUID } from "node:crypto";
import { pipeline } from "node:stream";
import { availableTiers, shouldUseExactModel, THRESHOLDS } from "./config.mjs";
import { askJev } from "./router.mjs";
import { decide } from "./policy.mjs";
import { debug } from "./log.mjs";
import { writeDecision, markManual, dumpBody } from "./status.mjs";
import { upstreamHeaders, prewarmJev } from "./proxy.mjs";

const CHATGPT_BASE_URL = "https://chatgpt.com/backend-api/codex";
const API_BASE_URL = "https://api.openai.com/v1";
export const CODEX_AUTO_MODEL = "jev-router";
const DEFAULT_MODELS = {
  haiku: "gpt-5.6-luna",
  sonnet: "gpt-5.6-terra",
  opus: "gpt-5.6-sol",
  fable: "gpt-6-astra",
};
const MODEL_ENV = {
  haiku: "JEV_CODEX_FAST_MODEL",
  sonnet: "JEV_CODEX_BALANCED_MODEL",
  opus: "JEV_CODEX_STRONG_MODEL",
  fable: "JEV_CODEX_LONG_MODEL",
};

export const codexModelOf = (tier) => process.env[MODEL_ENV[tier]] ?? DEFAULT_MODELS[tier];

export function codexTierOf(model) {
  const configured = Object.keys(DEFAULT_MODELS).find((tier) => codexModelOf(tier) === model);
  if (configured) return configured;
  if (/(?:astra|fable|long)/i.test(model ?? "")) return "fable";
  if (/(?:sol|opus|strong|max|pro)/i.test(model ?? "")) return "opus";
  if (/(?:luna|haiku|fast|mini|nano)/i.test(model ?? "")) return "haiku";
  return /^gpt-/i.test(model ?? "") ? "sonnet" : null;
}

/** Exact GPT models in Codex's account catalog; configured ids are the cold-start fallback. */
export function codexModels(models = new Map()) {
  const available = [...models.values()]
    .filter((model) => model.slug !== CODEX_AUTO_MODEL && model.supported_in_api !== false)
    .map((model) => ({
      id: model.slug,
      tier: codexTierOf(model.slug),
      description: [
        model.display_name,
        model.description,
        model.context_window && `${model.context_window} context tokens`,
      ].filter(Boolean).join("; "),
    }))
    .filter((model) => model.tier);
  return available.length
    ? available
    : Object.keys(DEFAULT_MODELS).map((tier) => ({
        id: codexModelOf(tier),
        tier,
        description: codexModelOf(tier),
      }));
}

const modelForTier = (models, tier) =>
  models.find((model) => model.tier === tier)?.id ?? codexModelOf(tier);

const textOf = (content) => {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content
    .filter((item) => item?.type === "text" || item?.type === "input_text")
    .map((item) => item.text)
    .join("\n");
};

const cleanPrompt = (text) =>
  text
    .replace(/<system[-_]reminder>[\s\S]*?<\/system[-_]reminder>/gi, "")
    .replace(/<current_datetime>[\s\S]*?<\/current_datetime>/gi, "")
    .replace(/<environment_context>[\s\S]*?<\/environment_context>/gi, "")
    .trim();

export const isCodexAuxiliaryPrompt = (prompt) =>
  /^Generate a concise, single-line task title\b/i.test(prompt);

/** User text that starts a new Codex turn, or null for tool continuations. */
export function codexNewTurnPrompt(body) {
  if (!Array.isArray(body?.input)) return null;
  if (!body.input.some((item) => item?.type === "additional_tools")) return null;
  for (const item of [...body.input].reverse()) {
    if (item?.type === "function_call_output" || item?.type === "custom_tool_call_output") return null;
    if (item?.role !== "user") continue;
    const prompt = cleanPrompt(textOf(item.content));
    if (prompt && !isCodexAuxiliaryPrompt(prompt)) return prompt;
  }
  return null;
}

export function codexConversationKey(body) {
  const stable =
    body?.prompt_cache_key ??
    body?.client_metadata?.["x-codex-turn-metadata"] ??
    `${body?.instructions ?? ""}|${textOf(body?.input?.find((item) => item?.role === "user")?.content)}`;
  return createHash("sha1").update(String(stable)).digest("hex").slice(0, 12);
}

export function addJevModel(catalog) {
  if (!Array.isArray(catalog?.models) || catalog.models.some((model) => model.slug === CODEX_AUTO_MODEL)) {
    return catalog;
  }
  const template =
    catalog.models.find((model) => model.slug === codexModelOf("sonnet")) ??
    catalog.models.find((model) => model.visibility === "list") ??
    catalog.models[0];
  if (!template) return catalog;
  catalog.models.unshift({
    ...template,
    slug: CODEX_AUTO_MODEL,
    display_name: "Jev Router",
    description: "Jev picks the cheapest model that can complete each turn.",
    visibility: "list",
    supported_in_api: true,
    priority: 0,
    upgrade: null,
  });
  return catalog;
}

export function applyCodexTier(body, tier, models = new Map(), model = codexModelOf(tier)) {
  body.model = model;
  const info = models.get(model);
  const efforts = info?.supported_reasoning_levels?.map((level) => level.effort);
  if (body.reasoning?.effort && efforts?.length && !efforts.includes(body.reasoning.effort)) {
    body.reasoning.effort = info.default_reasoning_level;
  }
  return body;
}

export const upstreamFor = (
  headers,
  path = "",
  chatgptBaseURL = CHATGPT_BASE_URL,
  apiBaseURL = API_BASE_URL,
) => /\/models(?:\?|$)/.test(path) || headers["chatgpt-account-id"] ? chatgptBaseURL : apiBaseURL;

export function jevDecisionEvents({ tier, model = codexModelOf(tier), confidence, reason }) {
  const detail = Number.isFinite(confidence) ? `${reason}, confidence ${confidence.toFixed(2)}` : reason;
  const id = `jev-${randomUUID()}`;
  const text = reason.startsWith("jev-unavailable")
    ? `[Jev] unavailable; using ${model}. Add JEV_API_KEY=... to ~/.jev-router.env and restart jev-codex.`
    : `[Jev] routed this turn to ${model} (${detail}).`;
  const item = {
    type: "message",
    role: "assistant",
    id,
    phase: "commentary",
    content: [{ type: "output_text", text }],
  };
  const events = [
    { type: "response.output_item.added", item: { ...item, content: [] } },
    { type: "response.output_text.delta", item_id: id, delta: text },
    { type: "response.output_item.done", item },
  ];
  return events.map((event) => `event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`).join("");
}

const upstreamPath = (base, path) => `${new URL(base).pathname.replace(/\/$/, "")}${path}`;

// Conversations whose routing state is kept per proxy.
const MAX_CONVERSATIONS = 50;
// How much of a response is held back looking for the first event before giving up on
// showing the routing decision and passing the stream through as it is.
const MAX_INSPECT_BYTES = 64 * 1024;

export async function startCodexProxy({
  chatgptBaseURL = CHATGPT_BASE_URL,
  apiBaseURL = API_BASE_URL,
  route = askJev,
  statusId = "",
} = {}) {
  const states = new Map();
  const models = new Map();
  // Least recently used first and bounded, so a conversation still in use is not the one evicted
  // and a long Codex session does not grow this without limit.
  const stateOf = (key) => {
    const state = states.get(key);
    if (state) {
      states.delete(key);
      states.set(key, state);
    }
    return state;
  };
  const remember = (key, state) => {
    states.delete(key);
    if (states.size >= MAX_CONVERSATIONS) states.delete(states.keys().next().value);
    states.set(key, state);
  };

  const server = http.createServer((req, res) => {
    // Codex drops a request when the user interrupts it. Unless that reaches the API, it keeps
    // generating, and billing, the rest of a response nobody will read.
    let upstream = null;
    res.on("close", () => {
      if (!res.writableFinished) upstream?.destroy();
    });

    const chunks = [];
    req.on("data", (chunk) => chunks.push(chunk));
    req.on("end", async () => {
      let out = Buffer.concat(chunks);
      let routing;
      if (req.method === "POST" && /\/responses(?:\?|$)/.test(req.url ?? "")) {
        let body;
        let known;
        try {
          body = JSON.parse(out.toString());
          dumpBody(body);
          if (body.model === CODEX_AUTO_MODEL) {
            const key = codexConversationKey(body);
            known = stateOf(key);
            const candidates = codexModels(models).filter((model) =>
              availableTiers().includes(model.tier),
            );
            const available = [...new Set(candidates.map((model) => model.tier))];
            const currentModel = known?.model ?? modelForTier(candidates, THRESHOLDS.uncertainDefault);
            const current = codexTierOf(currentModel) ?? THRESHOLDS.uncertainDefault;
            const prompt = codexNewTurnPrompt(body);
            const explaining = prompt?.includes("<jev-explain>") || /^\$jev-explain\b/i.test(prompt ?? "");
            let tier = current;
            let model = currentModel;
            if (prompt && !explaining) {
              const contextTokens = Math.round(JSON.stringify(body.input).length / 4);
              const jev = await route({ prompt, current: currentModel, contextTokens, models: candidates });
              const chosen = candidates.find((candidate) => candidate.id === jev?.choice);
              const decision = decide({
                prompt,
                jev: jev && { ...jev, choice: chosen?.tier },
                current,
                available,
                // Nothing is cached yet for a conversation this proxy has not routed, so there
                // is no cache for a downgrade to throw away.
                contextTokens: known ? contextTokens : 0,
              });
              tier = decision.tier;
              model =
                shouldUseExactModel(decision.reason, chosen?.tier, tier)
                  ? chosen.id
                  : tier === current
                    ? currentModel
                    : modelForTier(candidates, tier);
              remember(key, { tier, model });
              routing = {
                prompt,
                tier,
                model,
                confidence: jev?.confidence ?? null,
                metrics: jev?.metrics ?? null,
                reason: decision.reason,
                jev: jev ? { request: jev.request, response: jev.response } : null,
                at: Date.now(),
              };
              writeDecision(statusId, routing);
              debug(`${key} ${current} -> ${tier} (${decision.reason}) | ${prompt.slice(0, 60)}`);
            }
            applyCodexTier(body, tier, models, model);
          } else {
            const prompt = codexNewTurnPrompt(body);
            const explaining = prompt?.includes("<jev-explain>") || /^\$jev-explain\b/i.test(prompt ?? "");
            // Keeps the routing history `$jev-explain` reads; a whole-file write blanked it.
            if (prompt && !explaining) markManual(statusId, body.model);
          }
          out = Buffer.from(JSON.stringify(body));
        } catch (err) {
          debug(`codex could not process body: ${err.message}`);
          // The sentinel is not a model the API knows; forwarding it fails the user's turn.
          if (body?.model === CODEX_AUTO_MODEL) {
            const fallback = THRESHOLDS.uncertainDefault;
            applyCodexTier(body, known?.tier ?? fallback, models, known?.model ?? codexModelOf(fallback));
            out = Buffer.from(JSON.stringify(body));
            routing = undefined;
          }
        }
      }

      // The user gave up while Jev was being asked; there is nobody to send this turn for.
      if (res.destroyed) return;

      const base = upstreamFor(req.headers, req.url, chatgptBaseURL, apiBaseURL);
      const target = new URL(base);
      const transport = target.protocol === "http:" ? http : https;
      const headers = upstreamHeaders(req.headers, target.host, out);
      upstream = transport.request(
        {
          hostname: target.hostname,
          port: target.port || undefined,
          path: upstreamPath(base, req.url ?? "/"),
          method: req.method,
          headers,
        },
        (response) => {
          const responseHeaders = { ...response.headers };
          const isModels = req.method === "GET" && /\/models(?:\?|$)/.test(req.url ?? "");
          if (isModels) {
            const body = [];
            response.on("error", (err) => res.destroy(err));
            response.on("data", (chunk) => body.push(chunk));
            response.on("end", () => {
              let data = Buffer.concat(body);
              try {
                const catalog = addJevModel(JSON.parse(data.toString()));
                for (const model of catalog.models) models.set(model.slug, model);
                data = Buffer.from(JSON.stringify(catalog));
                delete responseHeaders["content-length"];
              } catch (err) {
                debug(`could not extend Codex model catalog: ${err.message}`);
              }
              res.writeHead(response.statusCode, responseHeaders);
              res.end(data);
            });
            return;
          }

          const inspectForDecision = routing && response.statusCode >= 200 && response.statusCode < 300;
          if (inspectForDecision) delete responseHeaders["content-length"];
          res.writeHead(response.statusCode, responseHeaders);
          // Ties the two streams together both ways: a client that leaves stops the upstream
          // read, and an upstream that drops mid-stream fails the client fast.
          const passThrough = () =>
            pipeline(response, res, (err) => {
              if (err) debug(`codex stream ended early: ${err.message}`);
            });
          if (!inspectForDecision) return void passThrough();

          // Hold the stream until its first event, slip the routing decision in after it, then
          // hand the rest to `pipeline` so backpressure applies again. Searched as bytes: the
          // separator is ASCII, and decoding chunk by chunk would split multi-byte characters.
          let pending = Buffer.alloc(0);
          const onEnd = () => {
            if (pending.length) {
              debug("codex decision display skip");
              res.write(pending);
            }
            res.end();
          };
          const onData = (chunk) => {
            pending = Buffer.concat([pending, chunk]);
            const end = pending.indexOf("\n\n");
            if (end < 0 && pending.length < MAX_INSPECT_BYTES) return;
            response.off("data", onData);
            response.off("end", onEnd);
            if (end < 0) {
              debug("codex decision display skip");
              res.write(pending);
            } else {
              const first = pending.subarray(0, end + 2);
              res.write(first);
              const isSSE = /^(?:event|data):/m.test(first.toString("utf8"));
              if (isSSE) res.write(jevDecisionEvents(routing));
              debug(`codex decision display ${isSSE ? "inject" : "skip"}`);
              res.write(pending.subarray(end + 2));
            }
            pending = Buffer.alloc(0);
            passThrough();
          };
          response.on("error", (err) => res.destroy(err));
          response.on("data", onData);
          response.on("end", onEnd);
        },
      );
      upstream.on("error", (err) => {
        // Destroyed on purpose after the client left; there is no one to tell.
        if (res.destroyed) return;
        debug(`codex upstream error: ${err.message}`);
        // Mid-stream, an error body would be appended to an event stream as garbage.
        if (res.headersSent) return void res.destroy(err);
        res.writeHead(502, { "content-type": "application/json" });
        res.end(JSON.stringify({ error: { message: err.message, type: "proxy_error" } }));
      });
      if (out.length) upstream.write(out);
      upstream.end();
    });
  });

  if (route === askJev) prewarmJev();
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return { port: server.address().port, close: () => server.close() };
}
