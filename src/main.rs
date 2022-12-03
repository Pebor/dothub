use std::{
    env::{self},
    fs::{self, ReadDir},
    io::Error,
    path::{Path, PathBuf},
    process,
};

use clap::{Arg, ArgMatches, Command};
use fork::{daemon, Fork};
use serde_derive::Deserialize;

#[derive(Debug)]
struct DotFolder {
    name: String,
    dots: Vec<Dot>,
    config: Option<DotConfig>,
}

#[derive(Debug)]
struct Dot {
    name: String,
    config: Option<DotConfig>,
}

#[derive(Debug, Deserialize, Clone)]
struct DotConfig {
    start: Option<String>,
    kill: Option<String>,
    reload: Option<String>,
    destination: String,
}

fn main() {
    // check if $HOME/.dothub exists, if not, create one
    let user_home = env::var("HOME").expect("No $HOME set!");
    let folder_path = user_home.clone() + "/.dothub";
    let folder_path = Path::new(&folder_path);

    if !folder_path.exists() {
        fs::create_dir(folder_path).expect("Couldn't create '.dothub' in your $HOME");
    }

    // go through .dothub and initialize all DotFolders with their Dots
    let mut dot_folders: Vec<DotFolder> = vec![];

    for dot_folder in fs::read_dir(folder_path).unwrap() {
        let dot_folder = dot_folder.expect("Couldn't read DotFolder.").path();

        if dot_folder.is_dir() {
            dot_folders.push(process_dotfolder(dot_folder));
        }
    }

    // helper functions
    let get_dot_info_from_args = |args: &ArgMatches| -> (&DotFolder, Option<&Dot>) {
        let dotfolder_arg = args.get_one::<String>("DotFolder").unwrap();
        let dot_arg = args.get_one::<String>("Dot");

        let dotfolder = match dot_folders.iter().find(|df| &df.name == dotfolder_arg) {
            Some(df) => df,
            None => panic!("No DotFolder named '{}'", &dotfolder_arg),
        };

        if let Some(dot_arg) = dot_arg {
            let dot = match dotfolder.dots.iter().find(|d| &d.name == dot_arg) {
                Some(d) => d,
                None => panic!("No Dot named '{}'", &dot_arg),
            };

            return (dotfolder, Some(dot));
        }

        (dotfolder, None)
    };

    //TODO: h, cleanup, somehow
    let get_config = |dotfolder: &DotFolder, dot: Option<&Dot>| -> DotConfig {
        if let Some(dot) = dot {
            if let Some(config) = &dot.config {
                config.clone()
            } else {
                dotfolder.config.as_ref().expect("yes").clone()
            }
        } else {
            dotfolder.config.as_ref().expect("yes").clone()
        }
    };

    // command logic
    let args = arguments();

    match args.subcommand() {
        Some(("set", set_matches)) => {
            let (dotfolder, dot) = get_dot_info_from_args(&set_matches);
            let dot = dot.unwrap();

            //TODO: backup old configs
            if let Some(conf) = &dotfolder.config {
                let conf_path = Path::new(&conf.destination);

                let dot_path = format!(
                    "{}/{}/{}",
                    folder_path.to_str().unwrap(),
                    dotfolder.name,
                    dot.name
                );
                let dot_path = Path::new(&dot_path);

                //TODO: use symlinks
                if conf_path.is_file() {
                    fs::copy(dot_path, conf_path).expect("Couldn't copy Dot to destination.");
                } else if conf_path.is_dir() {
                    delete_dir_contents(fs::read_dir(conf_path));

                    let mut options = fs_extra::dir::CopyOptions::new();
                    options.content_only = true;
                    fs_extra::dir::copy(dot_path, conf_path, &options)
                        .expect("Couldn't copy DotFolder over to destination.");
                }
            } else {
                panic!("DotFolder has to have a .dothub with at least 'destination' filled!")
            }
        }
        Some(("start", start_matches)) => {
            let (dotfolder, dot) = get_dot_info_from_args(&start_matches);
            let config = get_config(dotfolder, dot);

            if let Some(start_cmd) = &config.start {
                if let Ok(Fork::Child) = daemon(false, false) {
                    process::Command::new("/bin/bash")
                        .args(["-c", start_cmd])
                        .output()
                        .expect("Couldn't start Dot");
                }
            } else {
                panic!("No 'start' command specified in any .dothub .");
            }
        }
        Some(("kill", start_matches)) => {
            let (dotfolder, dot) = get_dot_info_from_args(&start_matches);
            let config = get_config(dotfolder, dot);

            if let Some(kill_cmd) = &config.kill {
                if let Ok(Fork::Child) = daemon(false, false) {
                    process::Command::new("/bin/bash")
                        .args(["-c", kill_cmd])
                        .output()
                        .expect("Couldn't kill Dot");
                }
            } else {
                panic!("No 'kill' command specified in any .dothub .");
            }
        }
        Some(("reload", start_matches)) => {
            let (dotfolder, dot) = get_dot_info_from_args(&start_matches);
            let config = get_config(dotfolder, dot);

            if let Some(reload_cmd) = &config.reload {
                if let Ok(Fork::Child) = daemon(false, false) {
                    process::Command::new("/bin/bash")
                        .args(["-c", reload_cmd])
                        .output()
                        .expect("Couldn't kill Dot");
                }
            } else if let (Some(start_cmd), Some(kill_cmd)) = (&config.start, &config.kill) {
                if let Ok(Fork::Child) = daemon(false, false) {
                    process::Command::new("/bin/bash")
                        .args(["-c", &format!("{} && {}", kill_cmd, start_cmd)])
                        .output()
                        .expect("Couldn't kill Dot");
                }
            } else {
                panic!("No 'reload' command specified in any .dothub .");
            }
        }
        Some(("edit", start_matches)) => { // Later, use symlink arch first
            let (dotfolder, dot) = get_dot_info_from_args(&start_matches);
            let config = get_config(dotfolder, dot);

            process::Command::new(env::var("EDITOR").expect("$EDITOR has to be set!"))
                .arg(config.destination)
                .output()
                .expect("Couldn't kill Dot");
        }
        _ => unreachable!(),
    }
}

fn process_dotfolder(path: PathBuf) -> DotFolder {
    let name = path.file_name().unwrap().to_str().unwrap().to_owned();
    let mut config: Option<DotConfig> = None;

    let dots_paths = path.read_dir().unwrap();
    let dots: Vec<Dot> = dots_paths
        .filter_map(|dot_path| {
            let dot_path = dot_path.expect("Couldn't read Dot.").path();
            let dot_path_name = dot_path.file_name().unwrap().to_str().unwrap();

            if dot_path.is_dir() {
                return Some(process_dot(dot_path));
            } else if dot_path.is_file() && dot_path_name == ".dothub" {
                let config_file = fs::read_to_string(dot_path)
                    .expect("Couldn't read .dothub .")
                    .replace("~", &env::var("HOME").expect("No $HOME set!"));

                config = Some(toml::from_str(&config_file).expect("Couldn't parse .dothub ."));
            }
            None
        })
        .collect();

    DotFolder { name, dots, config }
}

fn process_dot(path: PathBuf) -> Dot {
    let name = path.file_name().unwrap().to_str().unwrap().to_owned();
    let mut config: Option<DotConfig> = None;

    let dots_files = path.read_dir().unwrap();
    for dot_path in dots_files {
        let dot_path = dot_path.expect("Couldn't read Dot.").path();
        let dot_path_name = dot_path.file_name().unwrap().to_str().unwrap();

        if dot_path.is_file() && dot_path_name == ".dothub" {
            let config_file = fs::read_to_string(dot_path)
                .expect("Couldn't read .dothub .")
                .replace("~", &env::var("HOME").expect("No $HOME set!"));

            config = Some(toml::from_str(&config_file).expect("Couldn't parse .dothub ."));
            break;
        }
    }

    Dot { name, config }
}

fn delete_dir_contents(read_dir_res: Result<ReadDir, Error>) {
    if let Ok(dir) = read_dir_res {
        for entry in dir {
            if let Ok(entry) = entry {
                let path = entry.path();

                if path.is_dir() {
                    fs::remove_dir_all(path).expect("Failed to remove a dir");
                } else {
                    fs::remove_file(path).expect("Failed to remove a file");
                }
            };
        }
    };
}

fn arguments() -> clap::ArgMatches {
    Command::new("dothub")
        .about("Manage your dofiles from a comfortable hub!")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .author("Yours truly")
        .subcommand(
            Command::new("set")
                .about("Applies (copies) a Dot.")
                .arg(Arg::new("DotFolder").required(true))
                .arg(Arg::new("Dot").required(true))
        )
        .subcommand(
            Command::new("start")
                .about("Starts a Dot, DotFolder config used if Dot isn't specified, or there is no Dot config.")
                .arg(Arg::new("DotFolder").required(true))
                .arg(Arg::new("Dot"))
        )
        .subcommand(
            Command::new("kill")
                .about("Kills a Dot, DotFolder config used if Dot isn't specified, or there is no Dot config.")
                .arg(Arg::new("DotFolder").required(true))
                .arg(Arg::new("Dot"))
        )
        .subcommand(
            Command::new("reload")
                .about("Reloads a Dot, DotFolder config used if Dot isn't specified, or there is no Dot config.")
                .arg(Arg::new("DotFolder").required(true))
                .arg(Arg::new("Dot"))
        )
        .get_matches()
}
