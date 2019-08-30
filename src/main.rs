#[macro_use]
extern crate derive_builder;
#[macro_use]
extern crate failure;

#[macro_use]
extern crate clap;

extern crate rand;
// extern crate rustsync;

mod actions;
mod data_shape;
mod develope;
mod log_util;
mod rustsync;

use std::borrow::Cow::{self, Borrowed, Owned};

use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::config::OutputStreamType;
use rustyline::error::ReadlineError;
use rustyline::highlight::{Highlighter, MatchingBracketHighlighter};
use rustyline::hint::{Hinter, HistoryHinter};
use rustyline::{Cmd, CompletionType, Config, Context, EditMode, Editor, Helper, KeyPress};
use std::time::Instant;

use data_shape::{load_dirs, Server};

struct MyHelper {
    completer: FilenameCompleter,
    highlighter: MatchingBracketHighlighter,
    hinter: HistoryHinter,
    colored_prompt: String,
}

impl Completer for MyHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &Context<'_>,
    ) -> Result<(usize, Vec<Pair>), ReadlineError> {
        self.completer.complete(line, pos, ctx)
    }
}

impl Hinter for MyHelper {
    fn hint(&self, line: &str, pos: usize, ctx: &Context<'_>) -> Option<String> {
        self.hinter.hint(line, pos, ctx)
    }
}

impl Highlighter for MyHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        default: bool,
    ) -> Cow<'b, str> {
        if default {
            Borrowed(&self.colored_prompt)
        } else {
            Borrowed(prompt)
        }
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Owned("\x1b[1m".to_owned() + hint + "\x1b[m")
    }

    fn highlight<'l>(&self, line: &'l str, pos: usize) -> Cow<'l, str> {
        self.highlighter.highlight(line, pos)
    }

    fn highlight_char(&self, line: &str, pos: usize) -> bool {
        self.highlighter.highlight_char(line, pos)
    }
}

impl Helper for MyHelper {}

fn main_client() {
    // env_logger::init();
    let config = Config::builder()
        .history_ignore_space(true)
        .completion_type(CompletionType::List)
        .edit_mode(EditMode::Emacs)
        .output_stream(OutputStreamType::Stdout)
        .build();

    let h = MyHelper {
        completer: FilenameCompleter::new(),
        highlighter: MatchingBracketHighlighter::new(),
        hinter: HistoryHinter {},
        colored_prompt: "".to_owned(),
    };
    let mut rl = Editor::with_config(config);
    rl.set_helper(Some(h));
    rl.bind_sequence(KeyPress::Meta('N'), Cmd::HistorySearchForward);
    rl.bind_sequence(KeyPress::Meta('P'), Cmd::HistorySearchBackward);
    if rl.load_history("history.txt").is_err() {
        println!("No previous history.");
    }
    let mut count = 1;
    loop {
        let p = format!("{}> ", count);
        rl.helper_mut().unwrap().colored_prompt = format!("\x1b[1;32m{}\x1b[0m", p);
        let readline = rl.readline(&p);
        match readline {
            Ok(line) => {
                rl.add_history_entry(line.as_str());
                println!("Line: {}", line);
                if line.starts_with("hash ") {
                    let (_s1, s2) = line.split_at(5);
                    actions::hash_file_sha1(s2).expect("hash should success.");
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("CTRL-C");
                break;
            }
            Err(ReadlineError::Eof) => {
                println!("CTRL-D");
                break;
            }
            Err(err) => {
                println!("Error: {:?}", err);
                break;
            }
        }
        count += 1;
    }
    rl.save_history("history.txt").unwrap();
}

fn main() -> Result<(), failure::Error> {
    use clap::App;
    use clap::ArgMatches;
    use clap::Shell;
    use log::*;
    use std::{fs, io};

    log_util::setup_logger(vec![""], vec![]);

    let yml = load_yaml!("17_yaml.yml");
    let app = App::from_yaml(yml);
    let m: ArgMatches = app.get_matches();

    let mut app1 = App::from_yaml(yml);

    match m.subcommand() {
        ("completions", Some(sub_matches)) => {
            let shell = sub_matches.value_of("shell_name").unwrap();
            app1.gen_completions_to(
                "ssh-client-demo",
                shell.parse::<Shell>().unwrap(),
                &mut io::stdout(),
            );
        }
        ("rsync", Some(sub_matches)) => {
            match sub_matches.subcommand() {
                ("sync-dirs", Some(sub_sub_matches)) => {
                    let server_config_path = sub_sub_matches.value_of("server-yml").unwrap();
                    let skip_sha1 = sub_sub_matches.is_present("skip-sha1");
                    match Server::load_from_yml(server_config_path) {
                        Ok(mut server) => {
                            if let Err(err) = server.sync_dirs(skip_sha1) {
                                error!("sync-dirs failed: {:?}", err);
                            }
                        }
                        Err(err) => {
                            error!("load_from_yml failed: {:?}", err);
                        }
                    }
                }
                ("signature", Some(sub_sub_matches)) => {
                    let file = sub_sub_matches.value_of("file").unwrap();
                    let block_size: Option<usize> = sub_sub_matches
                        .value_of("block-size")
                        .and_then(|s| s.parse().ok());
                    let out: Option<&str> = sub_sub_matches.value_of("out");
                    let start = Instant::now();
                    match rustsync::Signature::signature_a_file(file, block_size) {
                        Ok(mut sig) => {
                            if let Some(o) = out {
                                if let Err(err) = sig.write_to_file(o) {
                                    error!("rsync signature write_to_file failed: {:?}", err);
                                }
                            } else {
                                info!("igonre signature result.");
                            }
                        }
                        Err(err) => {
                            error!("rsync signature failed: {:?}", err);
                        }
                    }

                    println!("time costs: {:?}", start.elapsed().as_secs());
                }
                ("list-remote-files", Some(sub_sub_matches)) => {
                    let server_config_path = sub_sub_matches.value_of("server-yml").expect("should load sever yml.");
                    let skip_sha1 = sub_sub_matches.is_present("skip-sha1");
                    let start = Instant::now();
                    let mut server = Server::load_from_yml(server_config_path)?;

                    if let Some(out) = sub_sub_matches.value_of("out") {
                        let mut out = fs::OpenOptions::new().create(true).truncate(true).write(true).open(out)?;
                        server.list_remote_file(&mut out, skip_sha1)?;
                    } else {
                        server.list_remote_file(&mut io::stdout(), skip_sha1)?;
                    }        

                    println!("time costs: {:?}", start.elapsed().as_secs());
                }
                ("list-local-files", Some(sub_sub_matches)) => {
                    let server_config_path = sub_sub_matches.value_of("server-yml").expect("should load server yml.");
                    let skip_sha1 = sub_sub_matches.is_present("skip-sha1");
                    let start = Instant::now();

                    let server = Server::load_from_yml(server_config_path)?;

                    if let Some(out) = sub_sub_matches.value_of("out") {
                        let mut out = fs::OpenOptions::new().create(true).truncate(true).write(true).open(out)?;
                        server.load_dirs(&mut out, skip_sha1)?;
                    } else {
                        server.load_dirs(&mut io::stdout(), skip_sha1)?;
                    }        
                    println!("time costs: {:?}", start.elapsed().as_secs());
                }
                (_, _) => unimplemented!(), // for brevity
            }
        }
        ("repl", Some(_)) => {
            main_client();
        }
        (_, _) => unimplemented!(), // for brevity
    }
    Ok(())

    // if let Some(mode) = m.value_of("mode") {
    //     match mode {
    //         "vi" => println!("You are using vi"),
    //         "emacs" => println!("You are using emacs..."),
    //         _      => unreachable!()
    //     }
    // } else {
    //     println!("--mode <MODE> wasn't used...");
    // }
    // main_client();
}
