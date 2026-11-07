// UI behavior tests with small DOM/Worker stubs, not a rendered-browser test.
import test from "node:test";
import assert from "node:assert/strict";
import vm from "node:vm";
import {readFileSync} from "node:fs";
const html=readFileSync(new URL("../index.html",import.meta.url),"utf8");
const script=readFileSync(new URL("../app.js",import.meta.url),"utf8");
const indentScript=readFileSync(new URL("../indent.js",import.meta.url),"utf8");
function page({drafts={},brokenStorage=false,withEditor=false,confirmResult=true,search="",fetchImpl=async()=>{throw new TypeError("Network error");}}={}) {
  const nodes=new Map(),workers=[],downloads=[],opened=[],timers=new Map(),blobs=[];
  const storage=new Map(Object.entries(drafts));
  const confirmations=[];
  const requests=[];
  let focus=null, serial=0;
  class Element {
    value=""; textContent=""; innerHTML=""; disabled=false; checked=false;
    hidden=false; selectionStart=0; selectionEnd=0; listeners=new Map();
    addEventListener(type,handler) {this.listeners.set(type,handler);}
    emit(type,props={}) {return this.listeners.get(type)?.({target:this,preventDefault(){},...props});}
    click() {if (!this.disabled) this.emit("click");}
    focus() {focus=this;}
    select() {this.selectionStart=0;this.selectionEnd=this.value.length;}
    setRangeText(value,start,end) {this.value=this.value.slice(0,start)+value+this.value.slice(end);}
    setAttribute(key,value) {this[key]=value;}
    showModal() {this.open=true;}
    close() {this.open=false;}
  }
  for (const match of html.matchAll(/<[^>]*\bid="([^"]+)"[^>]*>/g)) {
    const e=new Element();
    e.disabled=/\bdisabled\b/.test(match[0]);e.checked=/\bchecked\b/.test(match[0]);
    e.hidden=/\bhidden\b/.test(match[0]);nodes.set(match[1],e);
  }
  nodes.get("mode").value="program";
  nodes.get("example").value="hello";
  nodes.get("report-title").value="Browser playground: unexpected behavior";
  const classes=new Set();
  class Worker {
    sent=[]; terminated=false;
    constructor(url,options) {this.url=url;this.options=options;workers.push(this);}
    postMessage(message) {this.sent.push(message);}
    terminate() {this.terminated=true;}
    reply(data) {this.onmessage?.({data});}
  }
  class TestURL extends URL {
    static createObjectURL(blob) {blobs.push(blob);return `blob:test-${blobs.length}`;}
    static revokeObjectURL() {}
  }
  const document={
    getElementById:id=>nodes.get(id),
    body:{classList:{toggle(name){if(classes.delete(name)) return false;classes.add(name);return true;}}},
    createElement:() => {const e=new Element();e.click=()=>downloads.push(e);return e;},
    execCommand:()=>{if(focus)focus.value="";},
  };
  let editorOptions, codeEditor;
  function CodeMirror() {}
  CodeMirror.defineMode=()=>{};
  CodeMirror.fromTextArea=(input,options)=>{
    editorOptions=options;
    const handlers=new Map(), settings={...options};
    const position=offset=>{
      const lines=input.value.slice(0,offset).split("\n");
      return {line:lines.length-1,ch:lines.at(-1).length};
    };
    const offset=pos=>input.value.split("\n").slice(0,pos.line).reduce((n,line)=>n+line.length+1,0)+pos.ch;
    codeEditor={
      getValue:()=>input.value,
      setValue(value) {input.value=value;input.selectionStart=input.selectionEnd=value.length;handlers.get("change")?.();},
      on:(name,handler)=>handlers.set(name,handler),
      getOption:name=>settings[name],setOption:(name,value)=>{settings[name]=value;},
      firstLine:()=>0,getLine:n=>input.value.split("\n")[n],
      listSelections:()=>[{from:()=>position(input.selectionStart),to:()=>position(input.selectionEnd)}],
      getRange:(from,to)=>input.value.slice(offset(from),offset(to)),
      replaceSelections(values) {
        const start=input.selectionStart,end=input.selectionEnd;
        input.value=input.value.slice(0,start)+values[0]+input.value.slice(end);
        input.selectionStart=input.selectionEnd=start+values[0].length;
        handlers.get("change")?.();
      },
      operation:fn=>fn(),scrollIntoView:()=>{},refresh:()=>{},focus:()=>{},
    };
    return codeEditor;
  };
  const context=vm.createContext({document,Worker,URL:TestURL,URLSearchParams,Blob,TextDecoder,AbortController,
    fetch:(...args)=>{requests.push(args);return fetchImpl(...args);},
    CodeMirror:withEditor ? CodeMirror : undefined,
    navigator:{userAgent:"test browser"},window:{location:{search},open:(...args)=>opened.push(args)},
    localStorage:{getItem:key=>{if(brokenStorage)throw Error();return storage.get(key)??null;},
      setItem:(key,value)=>{if(brokenStorage)throw Error();storage.set(key,value);}},
    confirm:message=>{confirmations.push(message);return confirmResult;},alert:()=>{},
    setTimeout:(callback,delay)=>{const id=++serial;timers.set(id,{callback,delay});return id;},
    clearTimeout:id=>timers.delete(id),
  });
  vm.runInContext(indentScript,context);
  vm.runInContext(script,context);
  return {get:id=>nodes.get(id),requests,confirmations,workers,downloads,opened,blobs,storage,classes,timers,editor:codeEditor,editorOptions};
}
function done(worker,props={}) {
  worker.reply({type:"done",ok:true,output:"",errors:"",seconds:0.01,...props});
}
test("interface opens with a real source editor and optional persisted drafts",()=>{
  const p=page();
  assert.match(p.get("source").value,/Hello from rfortran/);
  assert.doesNotMatch(html,/id="fast"/);
  assert.equal(p.workers.length,0);
  const saved=page({drafts:{"rfortran.web.source":"print *, 7","rfortran.web.stdin":"42"}});
  assert.equal(saved.get("source").value,"print *, 7");
  assert.equal(saved.get("stdin").value,"42");
  assert.doesNotThrow(()=>page({brokenStorage:true}));
});
test("Run uses a module worker; missing runtime is an honest recoverable error",()=>{
  const p=page();p.get("run").click();
  const worker=p.workers[0];
  assert.equal(worker.url,"worker.mjs");assert.equal(worker.options.type,"module");
  assert.ok(p.get("run").disabled);assert.equal(worker.sent[0].fast,undefined);
  worker.reply({type:"error",text:"Interpreter not connected"});
  assert.equal(p.get("errors").textContent,"Interpreter not connected");
  assert.equal(p.get("output").textContent,"");
  assert.ok(worker.terminated);assert.equal(p.get("run").disabled,false);
});
test("Stop ignores late messages and timeout terminates the worker",()=>{
  const p=page();p.get("run").click();const worker=p.workers[0];
  p.get("stop").click();done(worker,{output:"late output"});
  assert.ok(worker.terminated);assert.equal(p.get("output").textContent,"");
  p.get("run").click();
  [...p.timers.values()].find(t=>t.delay===30000).callback();
  assert.ok(p.workers[1].terminated);
  assert.match(p.get("errors").textContent,/30 seconds/);
});
test("REPL state comes from the adapter; source edits need replay or discard",()=>{
  const p=page();p.get("mode").value="repl";p.get("mode").emit("change");
  const worker=p.workers[0];assert.equal(worker.sent[0].type,"repl-init");
  done(worker,{repl:true,source:"",variables:"(no variables)"});
  p.get("repl-command").value="integer :: i";p.get("repl-submit").click();
  assert.equal(worker.sent.at(-1).command,"integer :: i");
  done(worker,{repl:true,source:"integer :: i\n",variables:"i (unset)"});
  assert.equal(p.get("variables").textContent,"i (unset)");
  p.get("source").value="integer :: j\n";p.get("source").emit("input");
  assert.ok(p.get("repl-submit").disabled);
  p.get("discard-edits").click();
  assert.equal(p.get("source").value,"integer :: i\n");
  assert.equal(p.get("repl-submit").disabled,false);
});
test("examples, draft persistence, Tab, and source download work without runtime",async()=>{
  const p=page();p.get("example").value="arrays";p.get("example").emit("change");
  assert.match(p.get("source").value,/dp = kind\(0.0d0\)/);
  assert.equal(p.storage.get("rfortran.web.source"),p.get("source").value);
  p.get("source").value="abc";p.get("source").selectionStart=3;p.get("source").selectionEnd=3;
  p.get("source").emit("keydown",{key:"Tab"});
  assert.equal(p.get("source").value,"abc   ");
  assert.equal(p.storage.get("rfortran.web.source"),"abc   ");
  p.get("download").click();assert.equal(p.downloads[0].download,"program.f90");
  assert.equal(await p.blobs[0].text(),"abc   ");
});

test("textarea fallback and REPL multiline input always indent three spaces",()=>{
  const p=page();const input=p.get("source");
  input.value="program p\ndo i=1,3";
  input.selectionStart=input.selectionEnd=input.value.length;
  input.emit("keydown",{key:"Enter"});
  assert.equal(input.value,"program p\ndo i=1,3\n   ");
  assert.equal(p.storage.get("rfortran.web.source"),input.value);
  const command=p.get("repl-command");command.value="do i=1,3";
  command.selectionStart=command.selectionEnd=command.value.length;
  command.emit("keydown",{key:"Enter",shiftKey:true});
  assert.equal(command.value,"do i=1,3\n   ");
  assert.equal(p.workers.length,0);
});

test("CodeMirror Enter explicitly indents DO and IF, including selection replacement",()=>{
  const p=page({withEditor:true});
  for (const opening of ["do i=1,3", "if (i>0) then"]) {
    p.editor.setValue(`program p\n${opening}`);
    p.editorOptions.extraKeys.Enter(p.editor);
    assert.equal(p.editor.getValue(),`program p\n${opening}\n   `);
  }
  p.editor.setValue("program p\ndo i=1,3\n   replace me\nend do");
  const input=p.get("source");
  input.selectionStart=input.value.indexOf("replace me");
  input.selectionEnd=input.selectionStart+"replace me".length;
  p.editorOptions.extraKeys["Shift-Enter"](p.editor);
  assert.match(p.editor.getValue(),/\n   \n   \nend do$/);
  p.editor.setOption("readOnly",true);
  const before=p.editor.getValue();
  p.editorOptions.extraKeys.Enter(p.editor);
  assert.equal(p.editor.getValue(),before);
});

test("REPL reports inferred-declaration settings and pending input",()=>{
  const p=page();p.get("mode").value="repl";p.get("mode").emit("change");
  const worker=p.workers[0];
  worker.reply({type:"status",text:"Running...",available:true,capabilities:{autoDeclare:true}});
  done(worker,{repl:true,source:"",variables:"(no variables)",autoDeclare:true,pending:true,waiting:null});
  assert.equal(p.get("auto-declare").checked,true);
  assert.match(p.get("repl-hint").textContent,/complete the block/);
  assert.equal(p.get("runtime-notice").hidden,true);
  p.get("repl-command").value="end do";p.get("repl-submit").click();
  assert.equal(worker.sent.at(-1).autoDeclare,true);
});
test("bug reports exclude source by default and open GitHub only on request",()=>{
  const p=page();p.get("source").value="private source";p.get("report").click();
  assert.equal(p.get("report-source").checked,false);
  assert.ok(!p.get("report-preview").value.includes("private source"));
  assert.equal(p.opened.length,0);
  p.get("report-source").checked=true;p.get("report-source").emit("change");
  assert.ok(p.get("report-preview").value.includes("private source"));
  p.get("report-open").click();
  assert.match(p.opened[0][0],/^https:\/\/github.com\/Beliavsky\/rfortran\/issues\/new\?/);
});
test("focus mode toggles and Ctrl+Enter starts the current program",()=>{
  const p=page();p.get("focus").click();assert.ok(p.classes.has("focus-mode"));
  p.get("focus").click();assert.equal(p.classes.has("focus-mode"),false);
  p.get("source").emit("keydown",{key:"Enter",ctrlKey:true});
  assert.equal(p.workers.length,1);
});


test("execution limits default, persist, validate and control the worker timer",()=>{
  const p=page();
  assert.equal(p.get("source-limit").value,"200000");
  assert.equal(p.get("time-limit").value,"30");
  p.get("source-limit").value="500000";
  p.get("time-limit").value="120";
  p.get("run").click();
  assert.equal(p.workers[0].sent[0].sourceLimit,500000);
  assert.ok(p.get("time-limit").disabled);
  const timeout=[...p.timers.values()].find(t=>t.delay===120000);
  assert.ok(timeout);
  timeout.callback();
  assert.match(p.get("errors").textContent,/120 seconds/);
  assert.equal(p.get("time-limit").disabled,false);
  assert.equal(p.storage.get("rfortran.web.source-limit"),"500000");
  const restored=page({drafts:Object.fromEntries(p.storage)});
  assert.equal(restored.get("time-limit").value,"120");
  for(const invalid of ["", "-1", "1.5", "Infinity", "2147484"]) {
    restored.get("time-limit").value=invalid;
    restored.get("run").click();
    assert.equal(restored.workers.length,0);
  }
});

test("zero limits disable the timeout and preserve Stop; bad saved values use defaults",()=>{
  const p=page({drafts:{"rfortran.web.source-limit":"bad","rfortran.web.time-limit":"-1"}});
  assert.equal(p.get("source-limit").value,"200000");
  assert.equal(p.get("time-limit").value,"30");
  p.get("source-limit").value="0";p.get("time-limit").value="0";
  p.get("run").click();
  assert.equal(p.workers[0].sent[0].sourceLimit,0);
  assert.equal(p.timers.size,0);
  assert.equal(p.get("stop").disabled,false);
  p.get("stop").click();
  assert.ok(p.workers[0].terminated);
});


function localFile(name, text) {return Object.assign(new Blob([text]),{name});}
async function loadUrl(p,url="https://example.org/my%20example.f90") {
  p.get("open-url").click();
  p.get("source-url").value=url;
  await p.get("url-form").emit("submit");
}

test("URL source fills both editors without execution and keeps its filename",async()=>{
  for(const withEditor of [false,true]) {
    const source="print *, 'from URL'\nend\n";
    const p=page({withEditor,fetchImpl:async()=>new Response(source,{headers:{"Content-Type":"text/plain"}})});
    await loadUrl(p);
    assert.equal(p.get("source").value,source);
    assert.equal(p.get("source-description").textContent,"my example.f90");
    assert.equal(p.get("url-dialog").open,false);
    assert.equal(p.workers.length,0);
    assert.equal(p.requests.length,1);
    assert.equal(p.requests[0][1].mode,"cors");
    assert.equal(p.requests[0][1].credentials,"omit");
    assert.equal(p.requests[0][1].referrerPolicy,"no-referrer");
    assert.equal(p.get("run").disabled,false);
    p.get("download").click();assert.equal(p.downloads[0].download,"my example.f90");
    assert.equal(p.storage.get("rfortran.web.source"),source);
  }
});

test("invalid source URLs and limits never issue a request",async()=>{
  const p=page();
  for(const url of ["not a URL","/relative.f90","file:///tmp/test.f90","data:text/plain,end","javascript:alert(1)","https://user:password@example.org/test.f90"]) {
    await loadUrl(p,url);
    assert.match(p.get("url-error").textContent,/HTTP or HTTPS/);
    assert.equal(p.requests.length,0);
  }
  p.get("source-limit").value="-1";
  await loadUrl(p);
  assert.match(p.get("url-error").textContent,/Source limit/);
  assert.equal(p.requests.length,0);
});

test("HTTP, HTML and CORS failures preserve source and explain alternatives",async()=>{
  const cases=[
    [async()=>new Response("missing",{status:404}),/HTTP 404/],
    [async()=>new Response("<html>file page</html>",{headers:{"Content-Type":"text/html; charset=UTF-8"}}),/raw\/plain-text.*Raw/],
    [async()=>{throw new TypeError("Failed to fetch");},/CORS.*Open source > Open file/],
    [async()=>new Response(new Uint8Array([255])),/Unable to load source/],
  ];
  for(const [fetchImpl,expected] of cases) {
    const p=page({fetchImpl}),source=p.get("source").value;
    await loadUrl(p);
    assert.match(p.get("url-error").textContent,expected);
    assert.equal(p.get("source").value,source);
    assert.equal(p.get("url-dialog").open,true);
    assert.equal(p.get("run").disabled,false);
    assert.equal(p.workers.length,0);
  }
});

test("URL loading enforces the character limit while streaming and accepts unlimited sources",async()=>{
  let reads=0,cancelled=false;
  const p=page({fetchImpl:async()=>({ok:true,headers:new Headers(),body:{getReader:()=>({
    async read(){reads++;return {done:false,value:new Uint8Array([49,50,51,52])};},
    async cancel(){cancelled=true;},releaseLock(){},
  })}})});
  const original=p.get("source").value;
  p.get("source-limit").value="3";
  await loadUrl(p);
  assert.equal(p.get("source").value,original);
  assert.match(p.get("url-error").textContent,/3-character limit/);
  assert.equal(reads,1);assert.equal(cancelled,true);
  const unlimited=page({fetchImpl:async()=>new Response("x".repeat(200001))});
  unlimited.get("source-limit").value="0";
  await loadUrl(unlimited);
  assert.equal(unlimited.get("source").value.length,200001);
});

test("URL loading protects edited drafts and supports cancellation before and during streaming",async()=>{
  const draft="print *, 'keep this'";
  const p=page({drafts:{"rfortran.web.source":draft},confirmResult:false,fetchImpl:async()=>new Response("end")});
  await loadUrl(p);
  assert.equal(p.confirmations.length,1);
  assert.equal(p.get("source").value,draft);
  assert.match(p.get("url-error").textContent,/edited source was kept/);
  for(const cancel of ["url-cancel","escape"]) {
    const pending=page({fetchImpl:(_url,{signal})=>new Promise((_resolve,reject)=>{
      signal.addEventListener("abort",()=>reject(Error("aborted")),{once:true});
    })});
    const original=pending.get("source").value;
    const task=loadUrl(pending);
    assert.ok(pending.get("run").disabled);assert.ok(pending.get("open-file").disabled);
    if(cancel==="escape") pending.get("url-dialog").emit("cancel");
    else pending.get("url-cancel").click();
    await task;
    assert.ok(pending.requests[0][1].signal.aborted);
    assert.equal(pending.get("source").value,original);
    assert.equal(pending.get("run").disabled,false);
    assert.equal(pending.get("url-error").textContent,"");
  }
  let started,signal;
  const reading=new Promise(resolve=>{started=resolve;});
  const streaming=page({fetchImpl:async(_url,options)=>{
    signal=options.signal;
    return new Response(new ReadableStream({start(controller){
      signal.addEventListener("abort",()=>controller.error(Error("aborted")),{once:true});
      started();
    }}));
  }});
  const task=loadUrl(streaming);
  await reading;
  streaming.get("url-cancel").click();
  await task;
  assert.ok(signal.aborted);
  assert.equal(streaming.get("run").disabled,false);
  assert.equal(streaming.get("url-error").textContent,"");
});

test("shareable source links load without running and preserve edited drafts",async()=>{
  const url="https://example.org/shared.f90?a=1&b=2";
  const search="?source="+encodeURIComponent(url)+"&run=1";
  const p=page({search,fetchImpl:async()=>new Response("print *, 42\nend\n")});
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(p.requests[0][0],url);
  assert.equal(p.get("source").value,"print *, 42\nend\n");
  assert.equal(p.workers.length,0);
  const saved=page({search,drafts:{"rfortran.web.source":"edited draft"},confirmResult:false,fetchImpl:async()=>new Response("end")});
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(saved.get("source").value,"edited draft");
  assert.equal(saved.confirmations.length,1);
  const invalid=page({search:"?source="+encodeURIComponent("javascript:alert(1)")});
  assert.equal(invalid.requests.length,0);
  assert.match(invalid.get("url-error").textContent,/HTTP or HTTPS/);
});

async function selectFile(p,file) {
  p.get("source-file").files=file ? [file] : [];
  p.get("source-file").value="fake-path";
  await p.get("source-file").emit("change");
}

test("Open file reads local text without running it and preserves filename and drafts",async()=>{
  for (const withEditor of [false,true]) {
    const p=page({withEditor});
    let pickerClicks=0;p.get("source-file").listeners.set("click",()=>pickerClicks++);
    p.get("open-file").click();assert.equal(pickerClicks,1);
    const source="program test\n   print *, 'local file'\nend\n";
    const file=localFile("my test.f90",source);
    await selectFile(p,file);
    assert.equal(p.get("source").value,source);
    assert.equal(p.get("source-description").textContent,"my test.f90");
    assert.equal(p.get("status").textContent,"Loaded my test.f90");
    assert.equal(p.get("source-file").value,"");
    assert.equal(p.workers.length,0);
    assert.equal(p.confirmations.length,0);
    p.get("download").click();
    assert.equal(p.downloads[0].download,"my test.f90");
    assert.equal(await p.blobs[0].text(),source);
    const restored=page({drafts:Object.fromEntries(p.storage)});
    assert.equal(restored.get("source").value,source);
    restored.get("download").click();assert.equal(restored.downloads[0].download,"my test.f90");
    await selectFile(p,file); // Same file can be selected again.
    assert.equal(p.get("source").value,source);
    p.get("mode").value="repl";p.get("mode").emit("change");
    assert.ok(p.get("open-file").hidden);
    done(p.workers[0],{repl:true,source:""});
    p.get("mode").value="program";p.get("mode").emit("change");
    assert.equal(p.get("source-description").textContent,"my test.f90");
    assert.equal(p.get("source").value,source);
    p.get("example").value="arrays";p.get("example").emit("change");
    p.get("download").click();assert.equal(p.downloads.at(-1).download,"program.f90");
  }
});

test("edited code requires confirmation and cancellation preserves the editor",async()=>{
  const p=page({drafts:{"rfortran.web.source":"print *, 'draft'"},confirmResult:false});
  await selectFile(p,localFile("other.f90","end\n"));
  assert.equal(p.confirmations.length,1);
  assert.match(p.confirmations[0],/Replace the edited source/);
  assert.equal(p.get("source").value,"print *, 'draft'");
  p.get("download").click();assert.equal(p.downloads[0].download,"program.f90");
  await selectFile(p,null);
  assert.equal(p.confirmations.length,1);
  const approved=page({drafts:{"rfortran.web.source":"edited"}});
  await selectFile(approved,localFile("new.f90","end\n"));
  assert.equal(approved.confirmations.length,1);
  assert.equal(approved.get("source").value,"end\n");
});

test("file loading enforces character limits, permits zero, and handles empty or invalid files",async()=>{
  const p=page(), original=p.get("source").value;
  p.get("source-limit").value="3";
  await selectFile(p,localFile("large.f90","1234"));
  assert.equal(p.get("source").value,original);
  assert.match(p.get("errors").textContent,/3-character limit/);
  assert.equal(p.get("open-file").disabled,false);
  // UTF-8 byte length exceeds three, but the decoded source has three characters.
  await selectFile(p,localFile("unicode.f90","\u03b1\u03b2\u03b3"));
  assert.equal(p.get("source").value,"\u03b1\u03b2\u03b3");
  p.get("source-limit").value="0";
  await selectFile(p,localFile("large.f90","x".repeat(200001)));
  assert.equal(p.get("source").value.length,200001);
  await selectFile(p,localFile("empty.f90",""));
  assert.equal(p.get("source").value,"");
  await selectFile(p,localFile("bad.f90",new Uint8Array([255])));
  assert.equal(p.get("source").value,"");
  assert.match(p.get("errors").textContent,/Unable to open bad.f90/);
  await selectFile(p,{name:"unreadable.f90",stream(){throw Error("file is unavailable");}});
  assert.match(p.get("errors").textContent,/file is unavailable/);
  p.get("source-limit").value="-1";
  await selectFile(p,localFile("end.f90","end"));
  assert.match(p.get("errors").textContent,/Source limit must/);
  assert.equal(p.get("source").value,"");
});

test("file reading stops at the limit and locks execution until reading finishes",async()=>{
  const p=page();p.get("source-limit").value="3";
  let resolveRead, reads=0, cancelled=false, released=false;
  const reader={read(){reads++;return new Promise(resolve=>{resolveRead=resolve;});},
    async cancel(){cancelled=true;},releaseLock(){released=true;}};
  const pending=selectFile(p,{name:"huge.f90",stream(){return {getReader:()=>reader};}});
  assert.ok(p.get("run").disabled);assert.ok(p.get("mode").disabled);
  assert.ok(p.get("open-file").disabled);
  p.get("run").click();assert.equal(p.workers.length,0);
  resolveRead({done:false,value:new Uint8Array([49,50,51,52])});
  await pending;
  assert.equal(reads,1);assert.ok(cancelled);assert.ok(released);
  assert.equal(p.get("run").disabled,false);assert.equal(p.get("mode").disabled,false);
  p.get("run").click();
  await selectFile(p,localFile("blocked.f90","end"));
  assert.notEqual(p.get("source").value,"end");
});
