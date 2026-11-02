// UI behavior tests with small DOM/Worker stubs, not a rendered-browser test.
import test from "node:test";
import assert from "node:assert/strict";
import vm from "node:vm";
import {readFileSync} from "node:fs";
const html=readFileSync(new URL("../index.html",import.meta.url),"utf8");
const script=readFileSync(new URL("../app.js",import.meta.url),"utf8");
const indentScript=readFileSync(new URL("../indent.js",import.meta.url),"utf8");
function page({drafts={},brokenStorage=false,withEditor=false}={}) {
  const nodes=new Map(),workers=[],downloads=[],opened=[],timers=new Map(),blobs=[];
  const storage=new Map(Object.entries(drafts));
  let focus=null, serial=0;
  class Element {
    value=""; textContent=""; innerHTML=""; disabled=false; checked=false;
    hidden=false; selectionStart=0; selectionEnd=0; listeners=new Map();
    addEventListener(type,handler) {this.listeners.set(type,handler);}
    emit(type,props={}) {this.listeners.get(type)?.({target:this,preventDefault(){},...props});}
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
  const context=vm.createContext({document,Worker,URL:TestURL,URLSearchParams,Blob,
    CodeMirror:withEditor ? CodeMirror : undefined,
    navigator:{userAgent:"test browser"},window:{open:(...args)=>opened.push(args)},
    localStorage:{getItem:key=>{if(brokenStorage)throw Error();return storage.get(key)??null;},
      setItem:(key,value)=>{if(brokenStorage)throw Error();storage.set(key,value);}},
    confirm:()=>true,alert:()=>{},
    setTimeout:(callback,delay)=>{const id=++serial;timers.set(id,{callback,delay});return id;},
    clearTimeout:id=>timers.delete(id),
  });
  vm.runInContext(indentScript,context);
  vm.runInContext(script,context);
  return {get:id=>nodes.get(id),workers,downloads,opened,blobs,storage,classes,timers,editor:codeEditor,editorOptions};
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
  assert.equal(input.value,"program p\ndo i=1,3\n      ");
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
    p.editor.setValue(`program p\n   ${opening}`);
    p.editorOptions.extraKeys.Enter(p.editor);
    assert.equal(p.editor.getValue(),`program p\n   ${opening}\n      `);
  }
  p.editor.setValue("program p\n   do i=1,3\n      replace me\n   end do");
  const input=p.get("source");
  input.selectionStart=input.value.indexOf("replace me");
  input.selectionEnd=input.selectionStart+"replace me".length;
  p.editorOptions.extraKeys["Shift-Enter"](p.editor);
  assert.match(p.editor.getValue(),/\n      \n      \n   end do$/);
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
