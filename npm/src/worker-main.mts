// The script createWorker() starts: the browser build, served over the
// protocol in protocol.mts. Not an entry point of its own.

import { type Port, serve } from "./protocol.mjs";
import * as web from "./web.mjs";

serve(self as unknown as Port, web, (wasm) => web.init(wasm as never));
