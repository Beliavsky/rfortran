// SPDX-License-Identifier: MIT
// Editing assistance only; Fortran parsing and validation stay in Rust.
var RfortIndent = (() => {
  const width = 3;
  const start = () => ({stack:[], pending:""});
  const copy = state => ({stack:[...state.stack], pending:state.pending});
  function code(line) {
    let result="", quote=null;
    for (let i=0;i<line.length;i++) {
      const c=line[i];
      if (quote) {
        result+=" ";
        if (c===quote) {
          if (line[i+1]===quote) {result+=" ";i++;}
          else quote=null;
        }
      } else if (c==="!") break;
      else if (c==="'" || c==='"') {quote=c;result+=" ";}
      else result+=c.toLowerCase();
    }
    return result.trim().replace(/^\d+\s+/,"").replace(/^\w+\s*:(?!:)\s*/,"");
  }
  function close(stack, kind) {
    if (!kind) {stack.pop();return;}
    const index=stack.lastIndexOf(kind);
    if (index>=0) stack.length=index;
  }
  // Returns the indentation of a statement and updates the following level.
  function statement(state,text) {
    const stack=state.stack;
    let level=stack.length;
    if (!text || /^\w+\s*(?:=|=>)/.test(text)) return level;
    const end=text.match(/^end\s*(program|module|submodule|function|subroutine|interface|block\s*data|block|associate|critical|team|select|where|forall|do|if|type|enum(?:eration)?)(?:\b|$)/);
    if (end || /^end\s*$/.test(text)) {
      close(stack,end ? end[1].replace(/\s/g,"") : null);
      return stack.length;
    }
    if (/^(?:case\b|class\s+(?:is\b|default\b)|type\s+is\b|rank\s*(?:\(|default\b))/.test(text)) {
      if (stack.at(-1)==="arm") stack.pop();
      level=stack.length;
      if (stack.at(-1)==="select") stack.push("arm");
      return level;
    }
    if (/^(?:else\b|elseif\b|elsewhere\b|contains\b)/.test(text)) return Math.max(0,level-1);
    let kind=null;
    if (/^if\s*\(.*\)\s*then\s*$/.test(text)) kind="if";
    else if (/^do(?:\s|$)/.test(text)) kind="do";
    else if (/^select\s*(?:case|type|rank)\b/.test(text)) kind="select";
    else if (/^(?:where|forall)\s*\(.*\)\s*$/.test(text)) kind=text.match(/^\w+/)[0];
    else if (/\b(function|subroutine)\s+[a-z_]\w*\s*(?:\(|$)/.test(text)) kind=text.match(/\b(function|subroutine)\s/)[1];
    else if (/^(?:abstract\s+)?interface\b/.test(text)) kind="interface";
    else if (/^type\s*(?:::|,.*::|[a-z_]\w*\s*$)/.test(text)) kind="type";
    else if (/^enum\s*,/.test(text)) kind="enum";
    else if (/^enumeration\s+type\b/.test(text)) kind="enumeration";
    else if (/^block\s*data\b/.test(text)) kind="blockdata";
    else if (/^change\s+team\b/.test(text)) kind="team";
    else if (/^(program|submodule|associate|critical|block)\b/.test(text)) kind=text.match(/^\w+/)[0];
    else if (/^module\s+(?!procedure\b)[a-z_]\w*/.test(text)) kind="module";
    if (kind) stack.push(kind);
    return level;
  }
  function advance(state,line) {
    let text=code(line);
    if (!text) return;
    if (state.pending) text=state.pending+" "+text.replace(/^&\s*/,"");
    state.pending="";
    if (text.endsWith("&")) {state.pending=text.slice(0,-1);return;}
    for (const part of text.split(";")) statement(state,part.trim());
  }
  function indent(state,next="") {
    if (state.pending) return (state.stack.length+1)*width;
    return statement(copy(state),code(next).split(";")[0].trim())*width;
  }
  function after(before,next="") {
    const state=start();
    for (const line of before.split(/\r?\n/)) advance(state,line);
    return " ".repeat(indent(state,next));
  }
  function newline(editor) {
    if (editor.getOption("readOnly")) return;
    editor.operation(() => {
      const values=editor.listSelections().map(selection => {
        const from=selection.from(), to=selection.to();
        const before=editor.getRange({line:editor.firstLine(),ch:0},from);
        const next=editor.getLine(to.line).slice(to.ch);
        return "\n"+after(before,next);
      });
      editor.replaceSelections(values,"end","+input");
      editor.scrollIntoView(null);
    });
  }
  function install(cm) {
    cm.defineMode("rfort-fortran",config => {
      const base=cm.getMode(config,"text/x-fortran");
      return {
        startState:()=>({base:cm.startState(base), ...start()}),
        copyState:state=>({base:cm.copyState(base,state.base), ...copy(state)}),
        token(stream,state) {
          if (stream.sol()) advance(state,stream.string);
          return base.token(stream,state.base);
        },
        indent:(state,text)=>indent(state,text),
        electricInput:/^\s*(?:end\b|end(?:do|if|where|forall|select|type|function|subroutine|program|module)\b|else\b|elseif\b|elsewhere\b|case\b|class\s+(?:is|default)\b|type\s+is\b|rank\s*(?:\(|default\b)|contains\b)/i,
        lineComment:"!",
      };
    });
  }
  return {width,start,copy,advance,indent,after,newline,install};
})();
if (typeof CodeMirror === "function") RfortIndent.install(CodeMirror);
