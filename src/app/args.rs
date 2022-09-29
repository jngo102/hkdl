use clap::{Parser, Subcommand};

#[derive(Subcommand, Debug)]
pub enum SubCommand {
    /// Install a mod
    Add { mod_name: String },
    /// Get detailed info on a mod
    Info { mod_name: String },
    /// Fetch a list of mods
    List { 
        #[clap(default_value = "")]
        filter: Option<String>,
    },
    /// Uninstall a mod
    Rm { mod_name: String },
    /// Set path to game directory
    SetPath {
        #[clap(value_hint = clap::ValueHint::DirPath)]
        path: String,
    },
    /// Update a mod
    Update { mod_name: String },
}

#[derive(Parser, Debug)]
pub struct Arguments {
    #[clap(subcommand)]
    pub cmd: SubCommand,
}