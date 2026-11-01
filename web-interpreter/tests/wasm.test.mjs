// Execute the actual WebAssembly binary through the same browser WASI shim.
import test from "node:test";
import assert from "node:assert/strict";
import {existsSync,readFileSync} from "node:fs";
import {createWasiRuntime} from "../runtime-adapter.mjs";
const artifact=process.env.RFORT_WASM_PATH || new URL("../runtime/rfort-browser.wasm",import.meta.url);
const available=existsSync(artifact);
if (process.env.RFORT_WASM_REQUIRED && !available) throw Error("Build the browser interpreter first.");
const compiled=available ? WebAssembly.compile(readFileSync(artifact)) : null;
const check=(name,fn) => test(name,{skip:!available},fn);
async function run(source,input="") {return createWasiRuntime(await compiled).runProgram({source,input});}
const words=s=>s.trim().split(/\s+/).join(" ");
async function session() {
  const runtime=createWasiRuntime(await compiled);
  const result=await runtime.startSession({});
  assert.equal(result.ok,true,result.errors);
  return runtime;
}
async function submit(runtime,command,options={}) {
  const result=await runtime.submit({command,...options});
  assert.equal(result.ok,true,`${command}: ${result.errors}`);
  return result;
}
check("WASM REPL retains scalars, arrays and expressions",async()=>{
  const r=await session();
  await submit(r,"integer :: n");await submit(r,"n=7");
  assert.equal(words((await submit(r,"n*n")).output),"49");
  await submit(r,"real :: x(3)");await submit(r,"x=[1.0,2.0,3.0]");
  const result=await submit(r,"sum(x)");
  assert.equal(words(result.output),"6.0");assert.match(result.variables,/n = 7/);
  assert.match((await submit(r,".shapes x")).output,/3/);
});
check("WASM REPL accumulates blocks and continuations",async()=>{
  const r=await session();await submit(r,"integer :: i,n");await submit(r,"n=0");
  assert.equal((await submit(r,"do i=1,3")).pending,true);
  assert.equal((await submit(r,"n=n+i")).pending,true);
  assert.equal((await submit(r,"end do")).pending,false);
  assert.equal(words((await submit(r,"n")).output),"6");
  assert.equal((await submit(r,"n = 10 + &")).pending,true);
  await submit(r,"& 2");assert.equal(words((await submit(r,"n")).output),"12");
  await submit(r,"if (.true.) then");await submit(r,".cancel");
  assert.equal((await submit(r,"n")).pending,false);
});
check("WASM REPL rolls back runtime failures and does not replay input",async()=>{
  const r=await session();await submit(r,"integer :: n");await submit(r,"n=7");
  await submit(r,"real :: x");
  const failed=await r.submit({command:"n=9; print *,x"});
  assert.equal(failed.ok,false);assert.match(failed.errors,/uninitialized/);
  assert.equal(words((await submit(r,"n")).output),"7");
  const stopped=await r.submit({command:"n=9; stop"});
  assert.equal(stopped.ok,false);
  assert.equal(words((await submit(r,"n")).output),"7");
  await submit(r,"call random_number(x)");
  const first=(await submit(r,"x")).output;
  const unrelated=await submit(r,"n=n+1");assert.equal(unrelated.output,"");
  assert.equal((await submit(r,"x")).output,first);
  await submit(r,"call random_number(x)");
  assert.notEqual((await submit(r,"x")).output,first);
});
check("WASM REPL supports shortcuts and inferred arrays",async()=>{
  const r=await session();
  assert.equal((await submit(r,".auto on")).autoDeclare,true);
  await submit(r,"x=[10.0,20.0,30.0]");
  assert.equal(words((await submit(r,"sum(x)")).output),"60.0");
  await submit(r,"x=[5.0,6.0]");
  assert.match((await submit(r,".sizes x")).output,/2/);
  await submit(r,"const k=3");await submit(r,"let n=2*k");
  assert.equal(words((await submit(r,"n")).output),"6");
  assert.equal((await submit(r,".auto off")).autoDeclare,false);
});
check("WASM REPL replay exports a runnable named-END program",async()=>{
  const r=await session();await submit(r,"integer :: old");
  const result=await r.replay({source:'program p\nimplicit none\ninteger::n\nn=9\nprint *,n\nend program p ! trailing comment\n'});
  assert.equal(result.ok,true,result.errors);assert.equal(words(result.output),"9");
  assert.doesNotMatch(result.variables,/old/);
  const exported=await run(result.source);
  assert.equal(exported.ok,true,exported.errors);assert.equal(words(exported.output),"9");
  assert.equal(words((await submit(r,"n")).output),"9");
  const invalid=await r.replay({source:'integer::n\nn = )\nend'});
  assert.equal(invalid.ok,false);
  assert.equal((await submit(r,".vars")).output,"");
});
check("WASM runs PRINT, arrays, math and non-default kinds",async()=>{
  const result=await run(`implicit none
integer,parameter::dp=kind(0.0d0)
real(dp)::x(3)
x=[3.0_dp,4.0_dp,12.0_dp]
print *,sum(x),sqrt(sum(x*x))
end`);
  assert.equal(result.ok,true,result.errors);
  assert.equal(words(result.output),"19.0 13.0");
});
check("WASM reads supplied standard input",async()=>{
  const result=await run('integer::n\nread *,n\nprint *,n*n\nend','7\n');
  assert.equal(result.ok,true,result.errors);assert.equal(words(result.output),"49");
});
check("WASM diagnoses syntax errors with a source location",async()=>{
  const result=await run('implicit none\ninteger :: x\nx = )\nend');
  assert.equal(result.ok,false);assert.match(result.errors,/program.f90:3/);
});
check("WASM diagnoses unset variables at compile time and runtime",async()=>{
  const staticError=await run('implicit none\ninteger::x\nprint *,x\nend');
  assert.equal(staticError.ok,false);assert.match(staticError.errors,/uninitialized/);
  const dynamicError=await run('integer::i,x\nread *,i\nif(i>0) x=1\nprint *,x\nend','0\n');
  assert.equal(dynamicError.ok,false);assert.match(dynamicError.errors,/Runtime error:.*uninitialized/);
});
check("WASM STOP retains output, ends this run, and permits the next run",async()=>{
  const runtime=createWasiRuntime(await compiled);
  const stopped=await runtime.runProgram({source:'print *,42\nstop\nprint *,99\nend'});
  assert.equal(stopped.ok,true,stopped.errors);assert.equal(words(stopped.output),"42");
  const next=await runtime.runProgram({source:'print *,7\nend'});
  assert.equal(next.ok,true,next.errors);assert.equal(words(next.output),"7");
  const failure=await run('error stop "deliberate failure"\nend');
  assert.equal(failure.ok,false);assert.match(failure.errors,/deliberate failure/);
});
check("WASM supports a module and function in the submitted source",async()=>{
  const result=await run(`module m
contains
integer function twice(x)
integer,intent(in)::x
twice=2*x
end function
end module
program demo
use m
print *,twice(21)
end`);
  assert.equal(result.ok,true,result.errors);assert.equal(words(result.output),"42");
});
check("WASM limits output and recovers for a subsequent program",async()=>{
  const result=await run('integer::i\ndo i=1,12000\nprint *,repeat("x",100)\nend do\nend');
  assert.equal(result.ok,false);assert.match(result.errors,/Output limit reached/);
  assert.ok(result.output.length<=1_000_000);
  assert.equal((await run('print *,1\nend')).ok,true);
});
check("WASM virtual files support program I/O and disappear between runs",async()=>{
  const source='integer::u,n\nopen(newunit=u,file="numbers.txt",status="replace")\nwrite(u,*) 42\nclose(u)\nopen(newunit=u,file="numbers.txt",status="old")\nread(u,*) n\nclose(u)\nprint *,n\nend';
  const r=createWasiRuntime(await compiled);
  const result=await r.runProgram({source});
  assert.equal(result.ok,true,result.errors);assert.equal(words(result.output),"42");
  const next=await r.runProgram({source:'logical::found\ninquire(file="numbers.txt",exist=found)\nprint *,found\nend'});
  assert.equal(next.ok,true,next.errors);assert.equal(words(next.output),"F");
});
