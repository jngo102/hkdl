use clap::{Parser, Subcommand};

#[derive(Subcommand, Debug)]
pub enum SubCommand {
    /// Install a mod or enable the Modding API
    Add { query: String },
    /// Get detailed info on a mod
    Info { query: String },
    /// Fetch a list of mods
    List { 
        #[clap(default_value = "")]
        filter: Option<String>,
    },
    /// Uninstall a mod or disable the Modding API
    Rm { query: String },
    /// Set path to game directory
    SetPath {
        #[clap(value_hint = clap::ValueHint::DirPath)]
        path: String,
    },
    /// Update a mod or the Modding API
    Update { query: String },
}

#[derive(Parser, Debug)]
pub struct Arguments {
    #[clap(subcommand)]
    pub cmd: SubCommand,
}