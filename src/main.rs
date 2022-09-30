mod app;
mod mod_links;

use app::app::App;
use app::args::{Arguments, SubCommand};
use app::profile::Profile;
use clap::Parser;
use directories::BaseDirs;
use futures_util::StreamExt;
use log::{error, info, warn, LevelFilter};
use mod_links::api::*;
use mod_links::local::*;
use mod_links::remote::*;
use reqwest;
use serde_json;
use sha256::digest_file;
use simple_logging;
use std::cmp::min;
use std::env;
use std::fs;
use std::fs::File;
use std::io::{self, Cursor, Write};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{mpsc, Mutex, MutexGuard};
use sysinfo::{ProcessExt, System, SystemExt};
use tokio;
use tokio::runtime::Runtime;
use unzip::Unzipper;

/// An array of possible paths to the folder containing the Hollow Knight executable
static STATIC_PATHS: [&str; 6] = [
    "Program Files/Steam/steamapps/common/Hollow Knight",
    "Program Files (x86)/Steam/steamapps/common/Hollow Knight",
    "Program Files/GOG Galaxy/Games/Hollow Knight",
    "Program Files (x86)/GOG Galaxy/Games/Hollow Knight",
    "Steam/steamapps/common/Hollow Knight",
    "GOG Galaxy/Games/Hollow Knight",
];

/// An array of possible path suffixes to the Hollow Knight path's Managed folder
static SUFFIXES: [&str; 3] = [
    // GOG
    "Hollow Knight_Data/Managed",
    // Steam
    "hollow_knight_Data/Managed",
    // Mac
    "Contents/Resources/Data/Managed",
];

struct AppState(Mutex<App>);

const API_URL: &str = "https://raw.githubusercontent.com/hk-modding/modlinks/main/ApiLinks.xml";
const MOD_URL: &str = "https://raw.githubusercontent.com/hk-modding/modlinks/main/ModLinks.xml";
const SETTINGS_FOLDER: &str = "hkdl";

fn main() {
    let state = AppState(Default::default());
    exit_game();
    check_settings(&state);
    auto_detect(&state);
    fetch_mod_list(&state);
    parse_args(&state);
    exit_app(&state);
}

/// Automatically detect the path to Hollow Knight executable, else prompt the user to select its path.
/// # Arguments
/// * `state` - The state of the application
fn auto_detect(state: &AppState) {
    {
        let state = state.0.lock().unwrap();
        if state.settings.mods_path != "".to_string() {
            return;
        }
    }

    match env::consts::OS {
        "linux" | "mac" => {
            let mut state = state.0.lock().unwrap();
            match STATIC_PATHS.into_iter().find(|path| {
                let base_dir = BaseDirs::new().unwrap();
                let path_buf: PathBuf = [base_dir.data_dir().to_str().unwrap(), path]
                    .iter()
                    .collect();
                path_buf.exists()
            }) {
                Some(game_path) => {
                    let mut input = "".to_string();
                    print!(
                        "Game path detected at: {}. Is this correct? [y/n] ",
                        game_path
                    );
                    io::stdout().flush().unwrap();
                    io::stdin()
                        .read_line(&mut input)
                        .expect("Error: unable to read user input.");
                    while input.trim().to_lowercase() != "y" && input.trim().to_lowercase() != "n" {
                        println!("This is not a valid input. Please enter 'y' for 'yes' or 'n' for 'no'.");
                        print!(
                            "Game path detected at: {}. Is this correct? [y/n] ",
                            game_path
                        );
                        io::stdout().flush().unwrap();
                        io::stdin()
                            .read_line(&mut input)
                            .expect("Error: unable to read user input.");
                    }
                    if input.trim() == "y" {
                        match SUFFIXES.into_iter().find(|suffix| {
                            let path_buf: PathBuf = [game_path, suffix].iter().collect();
                            path_buf.exists()
                        }) {
                            Some(suffix) => {
                                let base_dir = BaseDirs::new().unwrap();
                                state.settings.mods_path = format!(
                                    "{}/{}/{}/Mods",
                                    base_dir.data_dir().to_str().unwrap(),
                                    game_path,
                                    suffix
                                )
                                .to_string();
                            }
                            None => {
                                error!("No managed path exists.");
                            }
                        }
                    } else {
                    }
                }
                None => {
                    println!("Could not detect your Hollow Knight installation. Please enter the folder that contains your Hollow Knight executable.");
                    enter_game_path(state)
                }
            }
        }
        "windows" => {
            let mut state = state.0.lock().unwrap();
            let mut drive_letter: String = "C:/".to_string();
            for i in 65u8..=90 {
                if PathBuf::from_str(format!("{}:/", i).as_str())
                    .unwrap()
                    .exists()
                {
                    drive_letter = format!("{}:/", i);
                }
            }
            match STATIC_PATHS.into_iter().find(|path| {
                let path_buf: PathBuf = [drive_letter.to_string(), path.to_string()]
                    .iter()
                    .collect();
                info!(
                    "Checking if path {} exists",
                    path_buf.clone().into_os_string().into_string().unwrap()
                );
                path_buf.exists()
            }) {
                Some(game_path) => {
                    let mut input = "".to_string();
                    print!(
                        "Game path detected at: {}. Is this correct? [y/n] ",
                        game_path
                    );
                    io::stdout().flush().unwrap();
                    io::stdin()
                        .read_line(&mut input)
                        .expect("Error: unable to read user input.");
                    while input.trim().to_lowercase() != "y" && input.trim().to_lowercase() != "n" {
                        println!("This is not a valid input. Please enter 'y' for 'yes' or 'n' for 'no'.");
                        print!(
                            "Game path detected at: {}. Is this correct? [y/n] ",
                            game_path
                        );
                        io::stdout().flush().unwrap();
                        io::stdin()
                            .read_line(&mut input)
                            .expect("Error: unable to read user input.");
                    }
                    if input.trim().to_lowercase() == "y" {
                        match SUFFIXES.into_iter().find(|suffix| {
                            let path_buf: PathBuf =
                                [drive_letter.as_str(), game_path, suffix].iter().collect();
                            info!(
                                "Checking managed path: {}",
                                path_buf.clone().into_os_string().into_string().unwrap()
                            );
                            path_buf.exists()
                        }) {
                            Some(suffix) => {
                                state.settings.mods_path = format!(
                                    "{}{}/{}/Mods",
                                    drive_letter.as_str(),
                                    game_path,
                                    suffix
                                );
                            }
                            None => error!("No managed path exists."),
                        }
                    } else if input.trim().to_lowercase() == "n" {
                        enter_game_path(state);
                    }
                }
                None => enter_game_path(state),
            }
        }
        _ => panic!("OS not supported"),
    }

    {
        let state = state.0.lock().unwrap();
        let mods_path = &state.settings.mods_path;
        if !PathBuf::from_str(mods_path.as_str()).unwrap().exists() {
            match fs::create_dir(mods_path.as_str()) {
                Ok(_) => info!("Successfully created mods directory."),
                Err(e) => error!("Error creating mods folder: {}", e),
            }
        }
    }
}

/// Load the settings JSON file into the settings object, or create the file if it does not exist
/// and open the log file
/// # Arguments
/// * `state` - The state of the application
fn check_settings(state: &AppState) {
    let base_dir = BaseDirs::new().unwrap();
    let settings_dir: PathBuf = [base_dir.data_dir().to_str().unwrap(), SETTINGS_FOLDER]
        .iter()
        .collect();
    if !settings_dir.exists() {
        match fs::create_dir(settings_dir.as_path()) {
            Ok(_) => info!("Created settings and log directory"),
            Err(e) => error!("Failed to create settings folder: {}", e),
        }
    }

    let settings_string = settings_dir.to_str().unwrap();
    let log_path = format!("{}/Log.txt", settings_string);
    match simple_logging::log_to_file(log_path.as_str(), LevelFilter::Info) {
        Ok(_) => info!("Opened logger at: {}", log_path.as_str()),
        Err(e) => {
            println!("Failed to open logger: {}", e);
            return;
        }
    }

    let settings_path = format!("{}/Settings.json", settings_string);
    if PathBuf::from_str(settings_path.as_str()).unwrap().exists() {
        let mut state = state.0.lock().unwrap();
        let settings_raw_text = fs::read_to_string(settings_path).unwrap();
        state.settings = serde_json::from_str(settings_raw_text.as_str()).unwrap();
    }
}

/// Create a new profile and save it to settings
/// # Arguments
/// * `state` - The state of the application
fn create_profile(state: &AppState) {
    let mut app_state = state.0.lock().unwrap();

    let mut profile_name = "".to_string();
    let mut mod_names = Vec::new();

    print!("Enter the name of the new profile: ");
    io::stdout().flush().unwrap();
    io::stdin().read_line(&mut profile_name).unwrap();
    let mut profile_exists = false;
    for profile in app_state.settings.profiles.iter() {
        if profile.name == profile_name {
            profile_exists = true;
        }
    }
    while profile_name == "".to_string() || profile_exists {
        if profile_name == "".to_string() {
            println!("Profile name cannot be empty.");
        } else if profile_exists {
            println!("Profile {:?} already exists.", profile_name);
        }

        print!("Enter the name of the new profile: ");
        io::stdout().flush().unwrap();
        io::stdin().read_line(&mut profile_name).unwrap();

        profile_exists = false;
        for profile in app_state.settings.profiles.iter() {
            if profile.name == profile_name {
                profile_exists = true;
            }
        }
    }

    profile_name = profile_name.trim().to_string();

    print!("Enter the name of a mod to include in the profile (leave blank to finish): ");
    io::stdout().flush().unwrap();
    let mut mod_name = "".to_string();
    io::stdin().read_line(&mut mod_name).unwrap();
    mod_name = mod_name.trim().replace(" ", "").to_lowercase();
    let mut manifest_name = "".to_string();
    while mod_name != "".to_string() {
        let mut mod_exists = app_state
            .settings
            .mod_links
            .manifests
            .iter()
            .any(|manifest| {
                manifest_name = manifest.name.clone();
                manifest.name.replace(" ", "").to_lowercase() == mod_name
            });
        while !mod_exists && mod_name != "".to_string() {
            println!("Mod {:?} does not exist.", mod_name);
            print!("Enter the name of a mod to include in the profile (leave blank to finish): ");
            io::stdout().flush().unwrap();
            mod_name = "".to_string();
            io::stdin().read_line(&mut mod_name).unwrap();
            mod_name = mod_name.trim().replace(" ", "").to_lowercase();
            mod_exists = app_state
                .settings
                .mod_links
                .manifests
                .iter()
                .any(|manifest| {
                    manifest_name = manifest.name.clone();
                    manifest.name.replace(" ", "").to_lowercase() == mod_name
                });
        }
        mod_names.push(manifest_name.clone());
        print!("Enter the name of a mod to include in the profile (leave blank to finish): ");
        io::stdout().flush().unwrap();
        mod_name = "".to_string();
        io::stdin().read_line(&mut mod_name).unwrap();
        mod_name = mod_name.trim().replace(" ", "").to_lowercase();
    }

    app_state.settings.profiles.push(Profile {
        name: profile_name,
        mods: mod_names,
    });
}

/// Delete a profile from settings
/// # Arguments
/// * `state` - The state of the application
fn delete_profile(state: &AppState) {
    let mut app_state = state.0.lock().unwrap();
    if app_state.settings.profiles.len() == 0 {
        println!("No profiles to delete.");
        return;
    }

    print!("Enter the name of the profile to delete: ");
    io::stdout().flush().unwrap();
    let mut profile_name = "".to_string();
    io::stdin().read_line(&mut profile_name).unwrap();
    profile_name = profile_name.trim().to_string();
    let mut profile_exists = app_state
        .settings
        .profiles
        .iter()
        .any(|profile| profile.name == profile_name);
    while !profile_exists {
        println!("Profile {:?} does not exist.", profile_name);
        profile_name = "".to_string();
        print!("Enter the name of the profile to delete: ");
        io::stdout().flush().unwrap();
        io::stdin().read_line(&mut profile_name).unwrap();
        profile_name = profile_name.trim().to_string();
        profile_exists = app_state
            .settings
            .profiles
            .iter()
            .any(|profile| profile.name == profile_name);
    }
    (*app_state)
        .settings
        .profiles
        .retain(|p| p.name != profile_name);

    println!("Profile {:?} deleted.", profile_name);
}

/// Disables the Modding API
/// # Arguments
/// * `state` - The state of the application
fn disable_api(state: &AppState) {
    let mods_path: String;
    {
        let app_state = state.0.lock().unwrap();
        mods_path = app_state.settings.mods_path.clone();
    }
    let managed_path: PathBuf = [mods_path.as_str(), ".."].iter().collect();
    let assembly: PathBuf = [managed_path.to_str().unwrap(), "Assembly-CSharp.dll"]
        .iter()
        .collect();
    let vanilla_assembly: PathBuf = [
        managed_path.to_str().unwrap(),
        "Assembly-CSharp.dll.vanilla",
    ]
    .iter()
    .collect();
    let modded_assembly: PathBuf = [managed_path.to_str().unwrap(), "Assembly-CSharp.dll.modded"]
        .iter()
        .collect();
    if vanilla_assembly.exists() && !modded_assembly.exists() {
        match fs::rename(assembly.clone(), modded_assembly) {
            Ok(_) => info!("Successfully renamed Assembly-CSharp to modded assembly backup."),
            Err(e) => error!(
                "Failed to rename Assembly-CSharp to modded assembly backup: {}",
                e
            ),
        }

        match fs::rename(vanilla_assembly, assembly) {
            Ok(_) => info!("Successfully replaced modded Assembly-CSharp with vanilla assembly."),
            Err(e) => error!(
                "Failed to replace modded Assembly-CSharp with vanilla assembly: {}",
                e
            ),
        }
    }

    warn!("No action was taken.");
}

/// Move a mod folder into the Disabled folder if it is located in the Mods folder
/// # Arguments`
/// *`mod_name` - The name of the mod folder to be moved into the Disabled folder
/// * `state` - The state of the application
fn disable_mod(mod_name: String, state: &AppState) {
    info!("Disabling mod {:?}", mod_name);
    let mut app_state = state.0.lock().unwrap();
    let mods_path = &app_state.settings.mods_path;
    let mod_path: PathBuf = [mods_path.clone(), mod_name.clone()].iter().collect();
    let disabled_mods_path: PathBuf = [mods_path.to_string(), "Disabled".to_string()]
        .iter()
        .collect();
    let disabled_mod_path: PathBuf = [
        mods_path.to_string(),
        "Disabled".to_string(),
        mod_name.clone(),
    ]
    .iter()
    .collect();
    if !disabled_mods_path.exists() {
        match fs::create_dir(disabled_mods_path.as_path()) {
            Ok(_) => info!("Successfully created Disabled folder."),
            Err(e) => error!("Failed to create Disabled folder: {}", e),
        }
    }
    if mod_path.exists() {
        match fs::rename(mod_path.as_path(), disabled_mod_path) {
            Ok(_) => info!("Successfully moved mod {} to Disabled folder.", mod_name),
            Err(e) => error!(
                "Failed to move mod directory {:?} to Disabled: {}",
                mod_path.to_str().unwrap(),
                e
            ),
        }
    } else {
        warn!("Path {:?} does not exist.", mod_path.to_str().unwrap());
    }

    let manifests = &app_state.settings.mod_links.manifests;
    for i in 0..manifests.len() {
        if app_state.settings.mod_links.manifests[i].name == mod_name {
            app_state.settings.mod_links.manifests[i].enabled = false;
        }
    }
}

/// Download a mod to disk from a provided URL
/// # Arguments
/// * `tx` - The channel to send the download progress to
/// * `name` - The name of the mod to be downloaded
/// * `url` - The download link of the mod
/// * `mods_path` - The path to the mods folder
async fn download_mod(tx: mpsc::Sender<u8>, name: String, url: String, mods_path: String) {
    let client = reqwest::Client::new();
    let result = client
        .get(url.clone())
        .send()
        .await
        .expect("Failed to download mod.");
    let total_size = result
        .content_length()
        .ok_or(format!("Failed to get content length from {}", url))
        .unwrap();
    let mod_path = format!("{}/{}", mods_path, name);

    if !PathBuf::from_str(mod_path.as_str()).unwrap().exists() {
        match fs::create_dir(mod_path.clone()) {
            Ok(_) => info!("Successfully created mod folder for {:?}.", name),
            Err(e) => error!("Failed to create mod folder for {:?}: {}", name, e),
        }
    }

    let extension = url.split(".").last().unwrap();
    let download_path: String;
    if extension == "zip" {
        download_path = format!("{}/temp.zip", mod_path.clone());
    } else {
        download_path = format!(
            "{}/{}",
            mod_path.clone(),
            url.clone().split("/").last().unwrap()
        );
    }

    {
        let mut file = File::create(download_path.clone()).unwrap();
        let mut downloaded: u64 = 0;
        let mut stream = result.bytes_stream();
        while let Some(item) = stream.next().await {
            let chunk = item.unwrap();
            file.write_all(&chunk).unwrap();
            let new = min(downloaded + (chunk.len() as u64), total_size);
            downloaded = new;
            tx.send((((new as f64) / (total_size as f64)) * 100.0).floor() as u8)
                .expect("Failed to send download progress.");
        }
    }

    /*let file_hash = digest_file(download_path.clone()).unwrap();
    if file_hash.to_lowercase() != mod_hash.to_lowercase() {
        error!("Failed to verify SHA256 of downloaded file for mod {:?}, re-downloading...", mod_name);
        install_mod(mod_name.clone(), mod_version, mod_hash, mod_link.clone()).await;
    } else {
        info!("Downloaded hash of {:?} matches with that on modlinks.", mod_name);
    }*/

    if extension == "zip" {
        let file = File::open(download_path.clone()).unwrap();
        let unzipper = Unzipper::new(file, mod_path);
        match unzipper.unzip() {
            Ok(_) => info!("Successfully unzipped contents of {}", download_path),
            Err(e) => error!("Failed to unzip contents of {}: {}", download_path, e),
        }

        fs::remove_file(download_path).unwrap();
    }
}

/// Move a mod folder out of the Disabled folder if it is there
/// # Arguments
/// * `mod_name` - The name of the mod folder to move out of the Disabled folder
/// * `state` - The state of the application
fn enable_mod(mod_name: String, state: &AppState) {
    info!("Enabling mod {:?}", mod_name);
    let mut app_state = state.0.lock().unwrap();
    let mods_path = &app_state.settings.mods_path;
    let mod_path: PathBuf = [mods_path.to_string(), mod_name.clone()].iter().collect();
    let disabled_mod_path: PathBuf = [
        mods_path.to_string(),
        "Disabled".to_string(),
        mod_name.clone(),
    ]
    .iter()
    .collect();
    if disabled_mod_path.exists() {
        match fs::rename(disabled_mod_path.as_path(), mod_path.as_path()) {
            Ok(_) => info!(
                "Successfully moved mod {} out of Disabled folder.",
                mod_name
            ),
            Err(e) => error!(
                "Failed to move mod directory {:?} from Disabled: {}",
                mod_path.to_str().unwrap(),
                e
            ),
        }
    } else {
        warn!("Path {:?} does not exist.", mod_path.to_str().unwrap());
    }

    (*app_state)
        .settings
        .mod_links
        .manifests
        .iter_mut()
        .for_each(|m| {
            if m.name == mod_name {
                m.enabled = true;
            }
        });
}

/// Manually select the path of the game's executable
/// # Arguments
/// * `app` - The mutex guarding the application state
fn enter_game_path(app: MutexGuard<App>) {
    warn!("Entering game path manually.");
    print!("Enter your game path: ");
    io::stdout().flush().unwrap();
    let mut entered_path = String::new();
    io::stdin()
        .read_line(&mut entered_path)
        .expect("Error: entered path is not valid.");
    set_game_path(app, entered_path);
}

fn set_game_path(mut app: MutexGuard<App>, mut game_path: String) {
    let mut path = PathBuf::from_str(game_path.trim()).unwrap();
    let mut mods_path = "".to_string();
    match SUFFIXES.into_iter().find(|suffix| {
        let path_buf: PathBuf = [path.clone(), PathBuf::from_str(suffix).unwrap()]
            .iter()
            .collect();
        info!(
            "Checking selected path: {}",
            path_buf.clone().to_str().unwrap()
        );
        path_buf.exists()
    }) {
        Some(suffix) => mods_path = format!("{}/{}/Mods", path.to_str().unwrap(), suffix),
        None => error!("No managed path found."),
    }
    while mods_path.as_str() == "" {
        println!("Path {} is not a valid game path.", path.to_str().unwrap());
        print!("Enter your game path: ");
        io::stdout().flush().unwrap();
        game_path = String::new();
        io::stdin()
            .read_line(&mut game_path)
            .expect("Error: entered path is not valid.");
        path = PathBuf::from_str(game_path.trim()).unwrap();
        match SUFFIXES.into_iter().find(|suffix| {
            let path_buf: PathBuf = [path.clone(), PathBuf::from_str(suffix).unwrap()]
                .iter()
                .collect();
            info!(
                "Checking selected path: {}",
                path_buf.clone().to_str().unwrap()
            );
            path_buf.exists()
        }) {
            Some(suffix) => mods_path = format!("{}/{}/Mods", path.to_str().unwrap(), suffix),
            None => error!("No managed path found."),
        }
    }
    (*app).settings.mods_path = mods_path;
    print_and_log(format!("Mods path is now: {}", app.settings.mods_path));
}

/// Gracefully exit application
fn exit_app(state: &AppState) {
    let state = state.0.lock().unwrap();
    let settings = state.settings.clone();
    let base_dir = BaseDirs::new().unwrap();
    let settings_dir: PathBuf = [base_dir.data_dir().to_str().unwrap(), SETTINGS_FOLDER]
        .iter()
        .collect();
    if !settings_dir.exists() {
        match fs::create_dir(settings_dir.as_path()) {
            Ok(_) => info!("Succesfully created settings folder."),
            Err(e) => error!("Failed to create settings folder: {}", e),
        }
    }
    let settings_path: PathBuf = [settings_dir.to_str().unwrap(), "Settings.json"]
        .iter()
        .collect();
    // Save or create a settings file
    if settings_path.exists() {
        let settings_file = File::options()
            .write(true)
            .truncate(true)
            .open(settings_path.as_path())
            .unwrap();
        match serde_json::to_writer_pretty(settings_file, &settings) {
            Ok(_) => info!("Successfully saved settings."),
            Err(e) => error!("Failed to save settings: {}", e),
        }
    } else {
        let mut settings_file = File::create(settings_path.as_path()).unwrap();
        let settings_string = serde_json::to_string_pretty(&state.settings).unwrap();
        match settings_file.write_all(settings_string.as_bytes()) {
            Ok(_) => info!("Successfully created new settings file."),
            Err(e) => error!("Failed to create new settings file: {}", e),
        }
    }
}

/// Close Hollow Knight before starting the installer
fn exit_game() {
    let system = System::new_all();
    for process in system.processes_by_name("hollow_knight") {
        match process.kill() {
            true => info!("Successfully killed hollow_knight process."),
            false => error!("Failed to kill hollow_knight process."),
        }
    }

    for process in system.processes_by_name("Hollow Knight") {
        match process.kill() {
            true => info!("Successfully killed Hollow Knight process."),
            false => error!("Failed to kill Hollow Knight process."),
        }
    }
}

/// Load and return the list of mods from https://raw.githubusercontent.com/hk-modding/modlinks/main/ModLinks.xml
/// # Arguments
/// * `state` - The state of the application
fn fetch_mod_list(state: &AppState) {
    let mut state = state.0.lock().unwrap();
    let client = reqwest::blocking::Client::new();
    match client.get(MOD_URL).send() {
        Ok(response) => {
            let content = response.text().expect("Failed to get content of mod list.");
            let mut remote_mod_links = RemoteModLinks::new();
            let mods_json: String;
            match quick_xml::de::from_str(content.as_str()) {
                Ok(value) => {
                    info!("Successfully parsed ModLinks XML");
                    remote_mod_links = value;
                }
                Err(e) => error!("Failed to parse ModLinks XML: {}", e),
            }

            let saved_manifests: Vec<LocalModManifest> = vec![];

            // If save mod links are empty, then this is a first run of the app.
            if saved_manifests.len() > 0 {
                for manifest in remote_mod_links.clone().manifests {
                    if !saved_manifests
                        .clone()
                        .into_iter()
                        .map(|m| serde_json::to_string(&m.name).unwrap())
                        .collect::<Vec<String>>()
                        .contains(&manifest.name)
                    {
                        // new_mods.push(manifest.name.clone());
                    }

                    if saved_manifests
                        .clone()
                        .into_iter()
                        .map(|m| serde_json::to_string(&m.name).unwrap())
                        .collect::<Vec<String>>()
                        .contains(&manifest.name)
                        && !saved_manifests
                            .clone()
                            .into_iter()
                            .map(|m| serde_json::to_string(&m.version).unwrap())
                            .collect::<Vec<String>>()
                            .contains(&manifest.version)
                    {
                        // outdated_mods.push(manifest.name);
                    }
                }
            }

            let mod_count = remote_mod_links.manifests.len();

            let mods_path = &state.settings.mods_path;
            let disabled_path: PathBuf = [mods_path.as_str(), "Disabled"].iter().collect();
            for i in 0..mod_count {
                let mod_name = &remote_mod_links.manifests[i].name;
                let mod_path: PathBuf = [mods_path.clone(), mod_name.clone()].iter().collect();
                let disabled_mod_path: PathBuf = [
                    disabled_path.clone().into_os_string().to_str().unwrap(),
                    mod_name.as_str(),
                ]
                .iter()
                .collect();
                if mod_path.exists() || disabled_mod_path.exists() {
                    remote_mod_links.manifests[i].installed = true;
                }
                if mod_path.exists() && !disabled_mod_path.exists() {
                    remote_mod_links.manifests[i].enabled = true;
                }
            }

            mods_json = serde_json::to_string_pretty(&remote_mod_links).unwrap();
            state.settings.mod_links = serde_json::from_str(mods_json.as_str()).unwrap();
        }
        Err(e) => error!("Failed to fetch mod links: {}", e),
    }
}

/// Download a copy of the Modding API and replace local files with its contents if
/// their hashes do not match; Also backs up the vanilla Assembly-CSharp.dll file.
/// # Arguments
/// * `mods_path` - The path to the mods folder
fn install_api(state: &AppState) {
    let app_state = state.0.lock().unwrap();
    let mods_path = &app_state.settings.mods_path;
    let client = reqwest::blocking::Client::new();
    let result = client
        .get(API_URL)
        .send()
        .expect("Failed to get response for ApiLinks.");
    let content = result.text().expect("Failed to get response string.");
    let mut api_links = ApiLinks::new();
    match quick_xml::de::from_str(content.as_str()) {
        Ok(value) => {
            info!("Successfully parsed API XML.");
            api_links = value;
        }
        Err(e) => error!("Failed to parse API XML: {}", e),
    }

    let managed_path: PathBuf = [mods_path.as_str(), ".."].iter().collect();
    let base_dir = BaseDirs::new().unwrap();
    let settings_dir: PathBuf = [base_dir.data_dir().to_str().unwrap(), SETTINGS_FOLDER]
        .iter()
        .collect();
    let temp_path: PathBuf = [
        settings_dir
            .into_os_string()
            .into_string()
            .unwrap()
            .as_str(),
        "..",
        "Temp",
    ]
    .iter()
    .collect();
    let api_url: String;
    match env::consts::OS {
        "linux" => {
            api_url =
                "https://github.com/hk-modding/api/releases/latest/download/ModdingApiLinux.zip"
                    .to_string()
        }
        "mac" => {
            api_url = "https://github.com/hk-modding/api/releases/latest/download/ModdingApiMac.zip"
                .to_string()
        }
        "windows" => {
            api_url = "https://github.com/hk-modding/api/releases/latest/download/ModdingApiWin.zip"
                .to_string()
        }
        _ => panic!("OS not supported."),
    }

    match reqwest::blocking::get(api_url) {
        Ok(response) => {
            let content = response.bytes().unwrap();
            let reader = Cursor::new(content);
            let unzipper = Unzipper::new(reader, temp_path.clone());
            match unzipper.unzip() {
                Ok(_) => info!("Successfully unzipped API to Temp folder."),
                Err(e) => error!("Failed to unzip API to Temp folder: {}", e),
            }
        }
        Err(e) => error!("Failed to get response: {}", e),
    }

    for file in api_links.manifest.files.files {
        let temp_file: PathBuf = [temp_path.to_str().unwrap(), file.as_str()]
            .iter()
            .collect();
        let local_file: PathBuf = [managed_path.to_str().unwrap(), file.as_str()]
            .iter()
            .collect();
        if !local_file.exists() {
            match fs::rename(temp_file, local_file) {
                Ok(_) => info!(
                    "Successfully moved temp file for {:?} to Managed folder.",
                    file
                ),
                Err(e) => error!(
                    "Failed to move temp file for {:?} to Managed folder: {}",
                    file, e
                ),
            }
        } else if digest_file(temp_file.clone()).unwrap()
            != digest_file(local_file.clone()).unwrap()
        {
            if file == "Assembly-CSharp.dll" {
                let vanilla_backup: PathBuf = [
                    managed_path.to_str().unwrap(),
                    "Assembly-CSharp.dll.vanilla",
                ]
                .iter()
                .collect();
                match fs::rename(local_file.clone(), vanilla_backup) {
                    Ok(_) => info!("Successfully backed up vanilla Assembly-CSharp."),
                    Err(e) => error!("Failed to backup vanilla Assembly-Csharp: {}", e),
                }
            }
            match fs::rename(temp_file, local_file) {
                Ok(_) => info!(
                    "Successfully replaced old local file for {:?} with new API file.",
                    file
                ),
                Err(e) => error!(
                    "Failed to replace old local file for {:?} with new API file: {}",
                    file, e
                ),
            }
        }
    }

    match fs::remove_dir_all(temp_path) {
        Ok(_) => info!("Successfully deleted Temp folder."),
        Err(e) => error!("Failed to delete Temp folder: {}", e),
    }
}

/// Download a mod to disk from a provided link
/// # Arguments
/// * `mod_name` - The name of the mod folder to be created
/// * `state` - The state of the application
fn install_mod(mut mod_name: String, state: &AppState) {
    info!("Installing mod {:?}", mod_name);

    let mut mod_link = "".to_string();

    let manifests: Vec<LocalModManifest>;
    {
        let app_state = state.0.lock().unwrap();
        manifests = app_state.settings.mod_links.manifests.clone();
    }

    for manifest in manifests {
        if manifest.name.replace(" ", "").to_lowercase() == mod_name.replace(" ", "").to_lowercase()
        {
            mod_name = manifest.name.clone();
            mod_link = manifest.link.link;
            manifest
                .dependencies
                .dependencies
                .iter()
                .for_each(|dependency| {
                    install_mod(dependency.to_string(), state);
                });
        }
    }

    if mod_link == "".to_string() {
        print_and_log(format!("Mod {:?} not found.", mod_name));
        return;
    }

    let mods_path: String;
    {
        let mut app_state = state.0.lock().unwrap();
        (*app_state).current_download_progress = 0;
        mods_path = app_state.settings.mods_path.clone();
    }

    let mod_path: PathBuf = [mods_path.as_str(), mod_name.as_str()].iter().collect();
    let disabled_mod_path: PathBuf = [mods_path.as_str(), "Disabled", mod_name.as_str()]
        .iter()
        .collect();
    if mod_path.exists() {
        warn!("Mod {:?} is already installed and enabled.", mod_name);
        return;
    } else if disabled_mod_path.exists() {
        warn!(
            "Mod {:?} already exists but is disabled, enabling it instead.",
            mod_name
        );
        enable_mod(mod_name.clone(), state);
        return;
    }

    let (tx, rx) = mpsc::channel();
    let tx = tx.clone();

    let mod_name_param = mod_name.clone();
    let runtime = Runtime::new().unwrap();
    runtime.block_on(async move {
        tokio::spawn(async {
            download_mod(tx, mod_name_param, mod_link, mods_path).await;
        });
    });

    {
        let mut app_state = state.0.lock().unwrap();

        while app_state.current_download_progress < 100 {
            print!(
                "Downloading mod {:?}: {}%\r",
                mod_name, app_state.current_download_progress
            );
            std::io::stdout().flush().unwrap();
            (*app_state).current_download_progress = rx.recv().unwrap();
        }

        println!(
            "Downloading mod {:?}: {}%!",
            mod_name, app_state.current_download_progress
        );

        for i in 0..app_state.settings.mod_links.manifests.len() {
            if app_state.settings.mod_links.manifests[i].name == mod_name {
                app_state.settings.mod_links.manifests[i].installed = true;
                app_state.settings.mod_links.manifests[i].enabled = true;
            }
        }
    }
}

/// Parse arguments passed to application
/// # Arguments
/// * `state` - The state of the application
fn parse_args(state: &AppState) {
    let args = Arguments::parse();
    match args.cmd {
        SubCommand::Add { mut query } => {
            query = query.replace(" ", "").to_lowercase();
            if query == "api" {
                enable_api(state);
            } else if query == "profile" {
                create_profile(state);
            } else {
                install_mod(query, state);
            }
        }
        SubCommand::Info { mut query } => {
            let app_state = state.0.lock().unwrap();
            query = query.replace(" ", "").to_lowercase();
            app_state
                .settings
                .mod_links
                .manifests
                .iter()
                .for_each(|manifest| {
                    if manifest.name.to_lowercase().replace(" ", "") == query {
                        println!("Mod:\t\t{}", manifest.name);
                        println!("Description:\t{}", manifest.description);
                        println!("Version:\t{}", manifest.version);
                        println!("SHA256:\t\t{}", manifest.link.sha256);
                        println!("Repository:\t{}", manifest.repository);
                        if manifest.dependencies.dependencies.len() > 0 {
                            println!("Dependencies:");
                            manifest
                                .dependencies
                                .dependencies
                                .iter()
                                .for_each(|dependency| {
                                    println!("\t- {}", dependency);
                                });
                        } else {
                            println!("Dependencies:\tNone");
                        }
                        match &manifest.tags {
                            Some(tags) => {
                                println!("Tags:");
                                tags.tags.iter().for_each(|tag| {
                                    println!("\t- {}", tag);
                                });
                            }
                            None => println!("Tags:\t\tNone"),
                        }
                        println!("Enabled:\t{}", manifest.enabled);
                        println!("Installed:\t{}", manifest.installed);
                    }
                });

            match app_state.settings.profiles.iter().find(|profile| profile.name.trim().to_lowercase() == query) {
                Some(profile) => {
                    println!("Profile: {}", profile.name);
                    profile.mods.iter().for_each(|mod_name| {
                        println!("\t- {}", mod_name);
                    });
                }
                None => error!("Failed to find a profile of the name {:?}", query),
            }
        }
        SubCommand::List { filter } => {
            let app_state = state.0.lock().unwrap();
            match filter {
                Some(filter) => match filter.as_str() {
                    "profiles" => {
                        app_state.settings.profiles.iter().for_each(|profile| {
                            if profile.name == app_state.settings.current_profile {
                                println!("*** {} ***", profile.name);
                            } else {
                                println!("{}", profile.name);
                            }
                        });
                    }
                    "installed" => {
                        app_state
                            .settings
                            .mod_links
                            .manifests
                            .iter()
                            .for_each(|manifest| {
                                if manifest.installed {
                                    println!("{}", manifest.name);
                                }
                            });
                    }
                    "enabled" => {
                        app_state
                            .settings
                            .mod_links
                            .manifests
                            .iter()
                            .for_each(|manifest| {
                                if manifest.enabled {
                                    println!("{}", manifest.name);
                                }
                            });
                    }
                    "disabled" => {
                        app_state
                            .settings
                            .mod_links
                            .manifests
                            .iter()
                            .for_each(|manifest| {
                                if !manifest.enabled && manifest.installed {
                                    println!("{}", manifest.name);
                                }
                            });
                    }
                    "uninstalled" => {
                        app_state
                            .settings
                            .mod_links
                            .manifests
                            .iter()
                            .for_each(|manifest| {
                                if !manifest.installed {
                                    println!("{}", manifest.name);
                                }
                            });
                    }
                    _ => {
                        app_state
                            .settings
                            .mod_links
                            .manifests
                            .iter()
                            .for_each(|manifest| {
                                if manifest.name.to_lowercase().contains(filter.as_str()) {
                                    println!("{}", manifest.name);
                                }
                            });
                    }
                },
                None => {
                    app_state
                        .settings
                        .mod_links
                        .manifests
                        .iter()
                        .for_each(|manifest| {
                            println!("{}", manifest.name);
                        });
                }
            }
        }
        SubCommand::Rm { mut query } => {
            query = query.replace(" ", "").to_lowercase();
            match query.as_str() {
                "*" => {
                    let manifests: Vec<LocalModManifest>;
                    {
                        let app_state = state.0.lock().unwrap();
                        manifests = app_state.settings.mod_links.manifests.clone()
                    }

                    manifests.iter().for_each(|manifest| {
                        if manifest.installed {
                            uninstall_mod(manifest.name.clone(), state);
                        }
                    });
                }
                "api" => {
                    disable_api(state);
                }
                "profile" => {
                    delete_profile(state);
                }
                _ => {
                    uninstall_mod(query, state);
                }
            }
        }
        SubCommand::SetPath { path } => {
            let app_state = state.0.lock().unwrap();
            set_game_path(app_state, path);
        }
        SubCommand::Update { query } => match query.as_str() {
            "profile" => set_profile(state),
            _ => {
                let filtered_mod_name = query.replace(" ", "").to_lowercase();
                println!("Filtered mod name: {}", filtered_mod_name);
            }
        },
    }
}

fn print_and_log(message: String) {
    println!("{}", message);
    info!("{}", message);
}

/// Enables the Modding API
/// # Arguments
/// * `state` - The state of the application
fn enable_api(state: &AppState) {
    let mods_path: String;
    {
        let app_state = state.0.lock().unwrap();
        mods_path = app_state.settings.mods_path.clone();
    }
    let managed_path: PathBuf = [mods_path.as_str(), ".."].iter().collect();
    let assembly: PathBuf = [managed_path.to_str().unwrap(), "Assembly-CSharp.dll"]
        .iter()
        .collect();
    let vanilla_assembly: PathBuf = [
        managed_path.to_str().unwrap(),
        "Assembly-CSharp.dll.vanilla",
    ]
    .iter()
    .collect();
    let modded_assembly: PathBuf = [managed_path.to_str().unwrap(), "Assembly-CSharp.dll.modded"]
        .iter()
        .collect();
    if modded_assembly.exists() && !vanilla_assembly.exists() {
        match fs::rename(assembly.clone(), vanilla_assembly) {
            Ok(_) => info!("Successfully renamed Assembly-CSharp to modded assembly backup."),
            Err(e) => error!(
                "Failed to rename Assembly-CSharp to modded assembly backup: {}",
                e
            ),
        }

        match fs::rename(modded_assembly, assembly) {
            Ok(_) => info!("Successfully replaced vanilla Assembly-CSharp with modded assembly."),
            Err(e) => error!(
                "Failed to replace vanilla Assembly-CSharp with modded assembly: {}",
                e
            ),
        }
        return;
    } else if !modded_assembly.exists() && !vanilla_assembly.exists() {
        warn!("Neither the modded or vanilla assembly backups exists, downloading API.");
        match fs::rename(assembly.clone(), vanilla_assembly) {
            Ok(_) => info!("Successfully renamed Assembly-CSharp to modded assembly backup."),
            Err(e) => error!(
                "Failed to rename Assembly-CSharp to modded assembly backup: {}",
                e
            ),
        }
        install_api(state);
        return;
    } else if modded_assembly.exists() && vanilla_assembly.exists() {
        warn!("Somehow, both assembly backups exist.");
        match fs::remove_file(modded_assembly) {
            Ok(_) => info!("Successfully removed modded backup."),
            Err(e) => error!("Failed to remove modded backup: {}", e),
        }
        match fs::remove_file(vanilla_assembly) {
            Ok(_) => info!("Successfully removed vanilla backup."),
            Err(e) => error!("Failed to remove vanilla backup: {}", e),
        }
        return;
    }

    warn!("No action was taken.");
}

/// Sets the current mod profile in settings
/// # Arguments
/// * `state` - The state of the application
fn set_profile(state: &AppState) {
    let mods_to_install: Vec<String>;
    let manifests: Vec<LocalModManifest>;
    {
        let mut app_state = state.0.lock().unwrap();
        let mut profile_name = String::new();
        print!("Enter profile name: ");
        io::stdout().flush().unwrap();
        io::stdin()
            .read_line(&mut profile_name)
            .expect("Failed to read line.");
        profile_name = profile_name.trim().to_string();
        let mut profile_exists = false;
        for profile in app_state.settings.profiles.iter() {
            if profile.name == profile_name {
                profile_exists = true;
                break;
            }
        }

        while !profile_exists {
            println!("Profile {:?} does not exist.", profile_name);
            profile_name = "".to_string();
            print!("Enter profile name: ");
            io::stdout().flush().unwrap();
            io::stdin()
                .read_line(&mut profile_name)
                .expect("Failed to read line.");
            profile_name = profile_name.trim().to_string();
            profile_exists = false;
            for profile in app_state.settings.profiles.iter() {
                if profile.name == profile_name {
                    profile_exists = true;
                    break;
                }
            }
        }

        app_state.settings.current_profile = profile_name.clone();

        match app_state
            .settings
            .profiles
            .iter()
            .find(|profile| profile.name == profile_name)
        {
            Some(profile) => mods_to_install = profile.mods.clone(),
            None => {
                error!("Failed to find profile {:?}.", profile_name);
                return;
            }
        }

        manifests = app_state.settings.mod_links.manifests.clone();
    }

    manifests.iter().for_each(|manifest| {
        if mods_to_install.contains(&manifest.name) && !manifest.enabled {
            install_mod(manifest.name.clone(), state);
        } else if !mods_to_install.contains(&manifest.name) && manifest.enabled {
            disable_mod(manifest.name.clone(), state);
        }
    });
}

/// Removes a mod folder from disk
/// # Arguments
/// * `mod_name` - The name of the mod folder
/// * `state` - The state of the application
fn uninstall_mod(mut mod_name: String, state: &AppState) {
    info!("Uninstalling mod {:?}", mod_name);
    {
        let manifests: Vec<LocalModManifest>;
        {
            let app_state = state.0.lock().unwrap();
            manifests = app_state.settings.mod_links.manifests.clone();
        }

        let app_state = state.0.lock().unwrap();
        let mods_path = &app_state.settings.mods_path;

        for manifest in manifests {
            if manifest.name.replace(" ", "").to_lowercase() == mod_name {
                mod_name = manifest.name;
            }
        }

        let mod_path: PathBuf = [mods_path.to_string(), mod_name.clone()].iter().collect();
        let disabled_mod_path: PathBuf = [
            mods_path.to_string(),
            "Disabled".to_string(),
            mod_name.clone(),
        ]
        .iter()
        .collect();
        if mod_path.exists() {
            match fs::remove_dir_all(mod_path.as_path()) {
                Ok(_) => info!("Successfully removed all contents for {}", mod_name),
                Err(e) => error!(
                    "Failed to remove mod directory {:?}: {}",
                    mod_path.to_str().unwrap(),
                    e
                ),
            }
        } else if disabled_mod_path.exists() {
            match fs::remove_dir_all(disabled_mod_path.as_path()) {
                Ok(_) => info!("Successfully removed all contents for {}", mod_name),
                Err(e) => error!(
                    "Failed to remove mod directory {:?}: {}",
                    disabled_mod_path.to_str().unwrap(),
                    e
                ),
            }
        } else {
            warn!("Path {:?} does not exist.", mod_path.to_str().unwrap());
        }
    }

    {
        let manifests: Vec<LocalModManifest>;
        {
            let app_state = state.0.lock().unwrap();
            manifests = app_state.settings.mod_links.manifests.clone();
        }
        let mut app_state = state.0.lock().unwrap();
        for i in 0..manifests.len() {
            if manifests[i].name == mod_name {
                app_state.settings.mod_links.manifests[i].installed = false;
                app_state.settings.mod_links.manifests[i].enabled = false;
            }
        }
    }
}
