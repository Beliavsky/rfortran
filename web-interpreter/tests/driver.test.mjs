import test from "node:test";
import assert from "node:assert/strict";
import {createDriver} from "../driver.mjs";
import {createRuntime} from "../runtime-adapter.mjs";

function fixture() {
  const events = [], calls = [];
  let instances = 0, destroyed = 0;
  const driver = createDriver({emit:m => events.push(m), loadRuntime:async () => {
    instances++;
    return {
      build:"test adapter", capabilities:{autoDeclare:true},
      runProgram:async args => { calls.push(args); return {ok:true, output:"42"}; },
      startSession:async () => ({ok:true, source:"", variables:"(no variables)"}),
      submit:async args => { calls.push(args); return {ok:true, source:"integer :: i\ni=7\n", variables:"i = 7", output:"7"}; },
      replay:async args => ({ok:false, source:args.source, errors:"test syntax error"}),
      dispose:async () => { destroyed++; },
    };
  }});
  return {driver,events,calls,counts:() => ({instances,destroyed})};
}
test("missing WASM produces an explicit error and no fabricated output", async t => {
  t.mock.method(globalThis,"fetch",async () => new Response("",{status:404}));
  const events=[];
  const driver=createDriver({loadRuntime:createRuntime,emit:e=>events.push(e)});
  await driver.handle({source:'print *, "hello"'});
  assert.equal(events.length,1);
  assert.equal(events[0].type,"error");
  assert.match(events[0].text,/assets are missing/);
  assert.equal(events[0].output,undefined);
});
test("each program run is fresh and unsupported options stay off",async () => {
  const f=fixture();
  await f.driver.handle({source:"print *,42",input:"7",fast:true});
  await f.driver.handle({source:"print *,42"});
  assert.deepEqual(f.counts(),{instances:2,destroyed:2});
  assert.equal(f.calls[0].input,"7");
  assert.equal(f.calls[0].fast,false);
  assert.equal(f.events.filter(e=>e.type==="done").length,2);
});
test("REPL submissions retain one session and return accepted source",async () => {
  const f=fixture();
  await f.driver.handle({type:"repl-init"});
  await f.driver.handle({type:"repl-submit",command:"i=7",autoDeclare:true});
  await f.driver.handle({type:"repl-submit",command:"i"});
  assert.deepEqual(f.counts(),{instances:1,destroyed:0});
  assert.equal(f.calls[0].autoDeclare,true);
  assert.match(f.events.at(-1).source,/integer :: i/);
  assert.equal(f.events.at(-1).variables,"i = 7");
});
test("replay replaces the old session and retains diagnostics",async () => {
  const f=fixture();
  await f.driver.handle({type:"repl-init"});
  await f.driver.handle({type:"repl-replay",source:"bad statement"});
  assert.deepEqual(f.counts(),{instances:2,destroyed:1});
  assert.equal(f.events.at(-1).ok,false);
  assert.equal(f.events.at(-1).source,"bad statement");
});
test("commands require a session and payload size is checked before loading",async () => {
  const f=fixture();
  await f.driver.handle({type:"repl-submit",command:"i=1"});
  await f.driver.handle({source:"x".repeat(200001)});
  await f.driver.handle({type:"bogus"});
  assert.deepEqual(f.counts(),{instances:0,destroyed:0});
  assert.ok(f.events.every(e=>e.type==="error"));
});
test("runtime exceptions clear state and allow a fresh run",async () => {
  const events=[];
  let loads=0;
  const driver=createDriver({emit:e=>events.push(e),loadRuntime:async () => {
    loads++;
    return {runProgram:async () => {throw new Error("test failure");}};
  }});
  await driver.handle({source:"stop"});
  await driver.handle({source:"stop"});
  assert.equal(loads,2);
  assert.equal(events.at(-1).text,"test failure");
});
