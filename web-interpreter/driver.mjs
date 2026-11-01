// SPDX-License-Identifier: MIT
// Worker protocol, independent of compiler sources and WebAssembly bindings.
const LIMIT = 200_000;
const OUTPUT_LIMIT = 1_000_000;
const operations = new Map([
  ["program", "runProgram"], ["repl-init", "startSession"],
  ["repl-submit", "submit"], ["repl-replay", "replay"],
]);
function text(value, label) {
  if (typeof value !== "string" || value.length > LIMIT)
    throw new Error(`${label} must be text of at most ${LIMIT} characters.`);
  return value;
}
function output(value) {
  const s = typeof value === "string" ? value : "";
  return s.length > OUTPUT_LIMIT ? s.slice(0, OUTPUT_LIMIT) + "\n[Output truncated]" : s;
}
export function createDriver({loadRuntime, emit, now = () => performance.now()}) {
  let runtime = null;
  let session = false;
  async function dispose() {
    const previous = runtime;
    runtime = null;
    session = false;
    await previous?.dispose?.();
  }
  async function handle(request) {
    const start = now();
    try {
      const type = request?.type ?? "program";
      const method = operations.get(type);
      if (!method) throw new Error("Unknown interpreter operation.");
      const repl = type !== "program";
      const input = text(request.input ?? "", "Standard input");
      const source = text(request.source ?? "", "Source");
      const command = text(request.command ?? "", "REPL command");
      if (type === "program" || type === "repl-init" || type === "repl-replay") await dispose();
      if (type === "repl-submit" && !session) throw new Error("Start a REPL session before submitting a command.");
      if (!runtime) {
        runtime = await loadRuntime();
        if (!runtime || typeof runtime[method] !== "function") throw new Error(`Interpreter adapter does not support ${method}.`);
        emit({type:"status", text:"Running...", available:true,
          build:runtime.build ?? "rfortran WebAssembly (version unavailable)",
          capabilities:runtime.capabilities ?? {}});
      }
      if (typeof runtime[method] !== "function") throw new Error(`Interpreter adapter does not support ${method}.`);
      const capabilities = runtime.capabilities ?? {};
      const result = await runtime[method]({source, command, input,
        fast:Boolean(capabilities.fast && request.fast),
        autoDeclare:Boolean(capabilities.autoDeclare && request.autoDeclare)});
      if (!result || typeof result.ok !== "boolean") throw new Error("Interpreter adapter returned an invalid result.");
      if (repl && typeof result.source !== "string") throw new Error("REPL adapter must return the accepted session source.");
      if (repl) session = true;
      emit({type:"done", repl, ok:result.ok, seconds:Math.max(0, (now()-start)/1000),
        output:output(result.output), errors:output(result.errors),
        source:repl ? result.source : undefined,
        variables:repl ? output(result.variables) : undefined,
        pending:Boolean(repl && result.pending),
        waiting:result.waiting,
        autoDeclare:typeof result.autoDeclare === "boolean" ? result.autoDeclare : undefined,
        suggestedEnds:repl && Array.isArray(result.suggestedEnds)
          ? result.suggestedEnds.filter(s => typeof s === "string").slice(0, 100) : []});
      if (!repl) await dispose();
    } catch (error) {
      try { await dispose(); } catch { /* Preserve the original diagnostic. */ }
      emit({type:"error", text:error instanceof Error ? error.message : String(error)});
    }
  }
  return {handle, dispose};
}
