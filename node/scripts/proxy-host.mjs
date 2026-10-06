// Hosts one proxy instance so two separate `claude` invocations share it, which is what an
// interactive session does, and so the conformance harness can drive any implementation's proxy
// the same way. Forwards to ANTHROPIC_BASE_URL when set, as jev-claude does. Prints the port,
// then stays up until killed.
import { loadEnv } from "../src/env.mjs";
import { startProxy } from "../src/proxy.mjs";

loadEnv();
const inherited = process.env.ANTHROPIC_BASE_URL;
const { port } = await startProxy(inherited ? { upstreamURL: inherited } : {});
console.log(`PORT=${port}`);
