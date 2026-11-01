// SPDX-License-Identifier: MIT
//! Browser transactions reuse native Session, shortcuts, parsing and snapshots.
//! WASI exits abort one transaction; JavaScript commits only a returned snapshot.
use super::*;
use serde::{Serialize,Deserialize};
#[derive(Default,Serialize,Deserialize)]
struct State {session:Session,pending:String}
#[derive(Deserialize)]
struct Request {
    operation:String,
    #[serde(default)] command:String,
    #[serde(default)] source:String,
    #[serde(default)] automatic:Option<bool>,
}

fn variables(session:&Session)->Result<String> {
    let view=inspect::View::new(session)?;
    let mut names=session.names.iter().collect::<Vec<_>>();names.sort();
    let mut lines=dynamic::variables(&session.declarations,&session.allocations);
    for name in names {lines.push(format!("{name} = {}",view.value(name,session)));}
    Ok(if lines.is_empty(){"(no variables)".into()}else{lines.join("\n")})
}
fn command(state:&mut State,line:&str,args:&crate::cli::Cli)->Result<()> {
    let command=shortcuts::command(line.trim());
    match command.as_str() {
        ":clear" | ":reset" | ":quit" | ":q"=>{*state=State::default();},
        ":cancel"=>state.pending.clear(),
        ":auto on"=>state.session.automatic=true,
        ":auto off"=>state.session.automatic=false,
        ":auto"=>println!("automatic declarations: {}",if state.session.automatic{"on"}else{"off"}),
        ":info"=>for line in dynamic::variables(&state.session.declarations,&state.session.allocations){println!("{line}");},
        ":list" | ":list -n"=>{
            let source=state.session.history.source(&state.session.names);
            if command==":list -n"{print!("{source}");}
            else{for (i,line) in source.lines().enumerate(){println!("{:4} {line}",i+1);}}
        },
        ":help"=>println!("Declare scalars or INTEGER/REAL/LOGICAL arrays, assign values, or enter an expression.\nMultiline IF/DO/SELECT CASE and & continuations work.\n.vars, .info, .shapes, .sizes, .list, .clear and .cancel inspect or reset the session.\nLET and CONST use native rfort shortcuts; Auto-declare enables inferred declarations.\nUse Download source to export a completed session. READ, files, modules and procedures are not supported in the REPL; use Program mode."),
        text if matches!(text.split_whitespace().next(),Some(":vars"|":shapes"|":sizes"))=>{
            let end=text.find(char::is_whitespace).unwrap_or(text.len());
            let op=&text[..end];let arguments=&text[end..];
            let view=inspect::View::new(&state.session)?;
            let mut names=shortcuts::selected(arguments,&state.session.names)?;
            if arguments.trim().is_empty() && op!=":vars"{names.retain(|n|view.shapes.contains_key(n));}
            for name in names {
                let value=if op==":vars"{view.value(&name,&state.session)}else{view.shape(&name,&state.session,op==":sizes")?};
                println!("{name} = {value}");
            }
        },
        text if text.starts_with(':')=>bail!("unknown browser REPL command `{text}`; use .help"),
        _=>submit_line(&mut state.session,&mut state.pending,line,args)?,
    }
    Ok(())
}

pub(crate) fn run(args:&crate::cli::Cli)->Result<()> {
    let request:Request=serde_json::from_slice(&std::fs::read("/request.json")?)?;
    let mut state:State=if request.operation=="submit" {
        serde_json::from_slice(&std::fs::read("/session.json")?)?
    }else{State::default()};
    if let Some(automatic)=request.automatic {state.session.automatic=automatic;}
    let result=(||->Result<()> {
        match request.operation.as_str() {
            "init"=>{},
            "submit"=>for line in request.command.lines(){command(&mut state,line,args)?;},
            "replay"=>state.session.submit(&request.source,args)?,
            _=>bail!("unknown browser REPL operation"),
        }
        Ok(())
    })();
    let ok=result.is_ok();
    if let Err(error)=result {eprintln!("Error: {}",crate::source_diagnostics::user_names(&format!("{error:#}")));}
    let mut source=state.session.history.source(&state.session.names);
    if !state.pending.is_empty() {
        if let Some(end)=source.rfind("end program") {source.insert_str(end,&format!("! Pending input (not executed yet):\n{}",state.pending));}
    }
    let variables=variables(&state.session)?;
    let response=serde_json::json!({"ok":ok,"source":source,"variables":variables,
        "pending":!state.pending.is_empty(),"waiting":if code(&state.pending).trim_end().ends_with('&'){Some("continuation")}else{None},
        "autoDeclare":state.session.automatic,"state":state});
    std::fs::write("/response.json",serde_json::to_vec(&response)?)?;
    Ok(())
}
