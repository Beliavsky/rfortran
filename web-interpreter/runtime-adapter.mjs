// SPDX-License-Identifier: MIT
// The actual Rust interpreter runs in a fresh WASI instance for each program.
import {WASI, File, OpenFile, ConsoleStdout, PreopenDirectory, wasi as errno} from "./vendor/wasi/index.js";
const encoder = new TextEncoder();
const OUTPUT_LIMIT = 1_000_000;
const FILE_LIMIT = 2_000_000;
let compiled;

function capture() {
  const chunks = [];
  let length = 0;
  return {
    fd:new ConsoleStdout(bytes => {
      if (length + bytes.length > OUTPUT_LIMIT) throw new Error("Output limit reached (1 MB).");
      chunks.push(bytes.slice()); length += bytes.length;
    }),
    text() {
      const bytes = new Uint8Array(length);
      let offset = 0;
      for (const chunk of chunks) {bytes.set(chunk, offset); offset += chunk.length;}
      return new TextDecoder().decode(bytes);
    },
  };
}

// Integration tests use this too: no Node-only execution path.
export function createWasiRuntime(module, {build="development"} = {}) {
  let snapshot = null;
  let view = {source:"", variables:"(no variables)", pending:false};
  const reset = () => { snapshot=null; view={source:"",variables:"(no variables)",pending:false}; };
  async function invoke({source="", input="", command="", autoDeclare}, operation="program") {
      const stdout=capture(), stderr=capture();
      const repl=operation!=="program";
      const files=new Map([["program.f90",new File(encoder.encode(source),{readonly:true})]]);
      if (repl) {
        files.set("request.json",new File(encoder.encode(JSON.stringify({operation,source,command,automatic:autoDeclare})),{readonly:true}));
        if (snapshot!==null) files.set("session.json",new File(encoder.encode(snapshot),{readonly:true}));
      }
      const host=new WASI(repl ? ["rfort-browser","repl"] : ["rfort-browser"], ["NO_COLOR=1", "TERM=dumb"], [
        new OpenFile(new File(encoder.encode(input))),stdout.fd,stderr.fd,
        new PreopenDirectory("/",files),
      ],{debug:false});
      let instance, code=1, failure="", fileWrites=0;
      // Virtual files never refer to host files. Bound their JS-side storage too.
      const imports={...host.wasiImport};
      for (const operation of ["fd_write","fd_pwrite"]) {
        const original=imports[operation];
        imports[operation]=(fd,iovs,count,...rest) => {
          if (fd>2) {
            const view=new DataView(instance.exports.memory.buffer);
            let size=0;
            for(let i=0;i<count;i++) size+=view.getUint32(iovs+8*i+4,true);
            if (fileWrites+size>FILE_LIMIT) return errno.ERRNO_NOSPC;
            fileWrites+=size;
          }
          return original(fd,iovs,count,...rest);
        };
      }
      const open=imports.path_open;
      imports.path_open=(...args) => host.fds.length>=128 ? errno.ERRNO_MFILE : open(...args);
      const resize=imports.fd_filestat_set_size;
      imports.fd_filestat_set_size=(fd,size) => size>BigInt(FILE_LIMIT) ? errno.ERRNO_NOSPC : resize(fd,size);
      const allocate=imports.fd_allocate;
      imports.fd_allocate=(fd,offset,size) => offset+size>BigInt(FILE_LIMIT) ? errno.ERRNO_NOSPC : allocate(fd,offset,size);
      try {
        instance=await WebAssembly.instantiate(module,{wasi_snapshot_preview1:imports});
        code=host.start(instance);
      } catch(error) {
        failure=error instanceof WebAssembly.RuntimeError ? `WebAssembly runtime error: ${error.message}` : error.message;
      }
      const errors=[stderr.text(),failure].filter(Boolean).join("\n");
      const base={ok:code===0 && !failure,output:stdout.text(),errors,exitCode:code};
      if (!repl) return base;
      const response=files.get("response.json");
      if (response && base.ok) {
        const result=JSON.parse(new TextDecoder().decode(response.data));
        if (typeof result.source!=="string" || typeof result.variables!=="string" || typeof result.ok!=="boolean" || !result.state)
          throw new Error("Invalid browser session response.");
        snapshot=JSON.stringify(result.state);
        view={source:result.source,variables:result.variables,pending:result.pending,
          waiting:result.waiting,autoDeclare:result.autoDeclare};
        return {...base,...view,ok:result.ok};
      }
      // No response means STOP/runtime failure aborted this transaction. Keep
      // only the previously committed Rust snapshot, never replay earlier code.
      return {...base,...view,ok:false,errors:errors || "Submission ended before its state could be committed."};
  }
  return {
    build:`rfortran ${build} / WASI interpreter`,
    capabilities:{fast:false,autoDeclare:true,autoEnd:false,repl:true},
    runProgram:args=>invoke(args),
    startSession:args=>{reset();return invoke(args,"init");},
    submit:args=>{
      if (snapshot===null) throw new Error("Start a REPL session first.");
      return invoke(args,"submit");
    },
    replay:args=>{reset();return invoke(args,"replay");},
    async dispose() {reset();},
  };
}

export async function createRuntime() {
  if (!compiled) compiled=(async () => {
    const response=await fetch(new URL("./runtime/build.json",import.meta.url),{cache:"no-cache"});
    if (!response.ok) throw new Error("Browser interpreter assets are missing. Run: python web-interpreter/build.py");
    const info=await response.json();
    const wasm=await fetch(new URL(`./runtime/rfort-browser.wasm?v=${encodeURIComponent(info.sha256)}`,import.meta.url));
    if (!wasm.ok) throw new Error("Unable to load rfort-browser.wasm. Rebuild with python web-interpreter/build.py.");
    return {module:await WebAssembly.compile(await wasm.arrayBuffer()),build:info.commit};
  })().catch(error => {compiled=null;throw error;});
  const {module,build}=await compiled;
  return createWasiRuntime(module,{build});
}
