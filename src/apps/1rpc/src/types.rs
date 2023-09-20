use std::prelude::v1::*;

use apps::getargs::{Opt, Options};

#[derive(Debug)]
pub struct Args {
    pub executable: String,
    pub addr: String,
    pub is_demo: bool,
    pub routes: String,
    pub tls: String,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            executable: "".into(),
            addr: "0.0.0.0:3400".into(),
            is_demo: true,
            routes: "config.json".into(),
            tls: "".into(),
        }
    }
}

impl Args {
    pub fn from_args(mut args: Vec<String>) -> Self {
        let mut out = Args::default();
        out.executable = args.remove(0);
        let mut opts = Options::new(args.iter().map(|a| a.as_str()));
        while let Some(opt) = opts.next_opt().expect("argument parsing error") {
            match opt {
                Opt::Short('a') => {
                    out.addr = opts.value().unwrap().parse().unwrap();
                }
                Opt::Short('d') => {
                    out.is_demo = opts.value().unwrap().parse().unwrap();
                }
                Opt::Short('r') => {
                    out.routes = opts.value().unwrap().parse().unwrap();
                }
                Opt::Short('t') | Opt::Long("tls") => {
                    out.tls = opts.value().unwrap().parse().unwrap();
                },
                _ => continue,
            }
        }
        out
    }
}
