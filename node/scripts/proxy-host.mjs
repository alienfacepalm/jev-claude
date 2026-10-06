// Hosts one proxy instance so two separate `claude` invocations share it, which is what an
// interactive session does. Prints the port, then stays up until killed.
import { loadEnv } from "../src/env.mjs";
import { startProxy } from "../src/proxy.mjs";

loadEnv();
const { port } = await startProxy();
console.log(`PORT=${port}`);
