use fork::{daemon, Fork};
use std::{
    collections::HashMap,
    env, fs,
    io::{self, Write},
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    process,
    time::Duration,
};

use anyhow::{bail, Context, Result};
use clap::{Arg, ArgMatches, Command};
use notify::{Config, PollWatcher, Watcher};
use serde_derive::Deserialize;

#[derive(Debug)]
struct Profile {
    name: String,
    start: Option<Vec<String>>,
    dots: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct ProfileParsable {
    start: Option<Vec<String>>,
    dots: Option<HashMap<String, String>>,
}

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
    reload_on_set: Option<bool>,
}

fn main() -> Result<()> {
    // check if $HOME/.dothub exists, if not, create one
    let user_home = env::var("HOME").context("No $HOME set!")?;
    let folder_path = user_home + "/.dothub";
    let folder_path = Path::new(&folder_path);

    if !folder_path.exists() {
        fs::create_dir(folder_path).context("Couldn't create '.dothub' in your $HOME")?;
    }

    let profiles_path = &folder_path.join("profiles");

    if !profiles_path.exists() {
        fs::create_dir(profiles_path).context("Couldn't create 'profiles' in your .dothub .")?;
    }

    // go through .dothub/profiles and initialize all profiles
    let mut profiles: Vec<Profile> = vec![];

    for profile_file in fs::read_dir(profiles_path).unwrap() {
        let profile_file = profile_file.expect("Couldn't read profile").path();

        if profile_file.is_file() {
            profiles.push(process_profile(profile_file)?);
        }
    }

    // go through .dothub and initialize all DotFolders with their Dots
    let mut dot_folders: Vec<DotFolder> = vec![];

    for dot_folder in fs::read_dir(folder_path).unwrap() {
        let dot_folder = dot_folder.expect("Couldn't read DotFolder.").path();

        if dot_folder.is_dir() && !dot_folder.ends_with("profiles") {
            dot_folders.push(process_dotfolder(&dot_folder)?);
        }
    }

    // helper functions
    let get_dot_info_from_arg = |arg: &String| -> Result<(&DotFolder, Option<&Dot>)> {
        let (mut dotfolder_arg, mut dot_arg) = (None, None);

        let location = arg;

        if let Some((df_arg, d_arg)) = location.split_once('/') {
            dotfolder_arg = Some(df_arg);
            if !d_arg.is_empty() {
                dot_arg = Some(d_arg);
            }
        } else {
            dotfolder_arg = Some(location);
        }

        let dotfolder_arg = dotfolder_arg.unwrap();

        let dotfolder = dot_folders
            .iter()
            .find(|df| df.name == dotfolder_arg)
            .with_context(|| format!("No Dotfolder named '{}'", &dotfolder_arg))?;

        if let Some(dot_arg) = dot_arg {
            let dot = dotfolder
                .dots
                .iter()
                .find(|d| d.name == dot_arg)
                .with_context(|| format!("No Dot named '{}'", &dot_arg))?;

            return Ok((dotfolder, Some(dot)));
        }

        Ok((dotfolder, None))
    };

    let get_active_config = |dot_info: (&DotFolder, Option<&Dot>)| -> Result<DotConfig> {
        let (dotfolder, dot) = dot_info;

        let df_config = match dotfolder.config.clone() {
            Some(x) => x,
            None => {
                bail!(
                    "DotFolder '{}' is required to have the field 'destination' filled in it's .dothub!",
                    &dotfolder.name
                );
            }
        };

        if let Some(dot) = dot {
            if let Some(config) = dot.config.clone() {
                // merge
                return Ok(DotConfig {
                    start: config.start.or(df_config.start),
                    kill: config.kill.or(df_config.kill),
                    reload: config.reload.or(df_config.reload),
                    destination: {
                        if config.destination.is_empty() {
                            df_config.destination
                        } else {
                            config.destination
                        }
                    },
                    reload_on_set: config.reload_on_set.or(df_config.reload_on_set),
                });
            }
        }

        Ok(df_config)
    };

    // commands
    let args = arguments();

    match args.subcommand() {
        Some(("set", set_matches)) => {
            let (dotfolder, dot) =
                get_dot_info_from_arg(set_matches.get_one::<String>("location").unwrap())?;
            let config = get_active_config((dotfolder, dot))?;
            let dot = dot.unwrap();

            let conf_path = Path::new(&config.destination);
            let dot_path = format!(
                "{}/{}/{}",
                folder_path.to_str().unwrap(),
                dotfolder.name,
                dot.name
            );
            let dot_path = Path::new(&dot_path);

            dot_set(&config, dot_path, conf_path)?;
        }
        Some(("watch", set_matches)) => {
            let (dotfolder, dot) =
                get_dot_info_from_arg(set_matches.get_one::<String>("location").unwrap())?;
            let config = get_active_config((dotfolder, dot))?;
            let dot = dot.unwrap();

            if dotfolder.config.is_some() {
                let conf_path = Path::new(&config.destination);
                let dot_path = format!(
                    "{}/{}/{}",
                    folder_path.to_str().unwrap(),
                    dotfolder.name,
                    dot.name
                );

                println!(
                    "You are now watching for changes in '{}'.
Once a change is detected (for example, edits), your Dot will be automatically reloaded.",
                    &dot_path
                );

                let dot_path = Path::new(&dot_path);

                dot_set(&config, dot_path, conf_path)?;

                // watch for directory changes (writes, moves, etc..)
                let (tx, rx) = std::sync::mpsc::channel();

                let mut watcher = PollWatcher::new(
                    tx,
                    Config::default().with_poll_interval(Duration::from_secs(1)),
                )
                .expect("Couldn't create watcher");

                watcher
                    .watch(dot_path, notify::RecursiveMode::Recursive)
                    .expect("Couldn't add Dot path to watcher.");

                for res in rx {
                    match res {
                        Ok(ev) => {
                            if ev.paths[0].is_file() {
                                dot_reload(&config.clone())?;
                            }
                        }
                        Err(e) => println!("watch error: {:?}", e),
                    }
                }
            } else {
                bail!("DotFolder has to have a .dothub with at least 'destination' filled!")
            }
        }
        Some(("list", _)) => {
            for df in dot_folders {
                println!("{}/", df.name);
                for d in df.dots {
                    println!("  {}", d.name);
                }
            }
        }
        Some(("start", matches)) => {
            let config = get_active_config(get_dot_info_from_arg(
                matches.get_one::<String>("location").unwrap(),
            )?)?;

            dot_start(&config)?;
        }
        Some(("kill", matches)) => {
            let config = get_active_config(get_dot_info_from_arg(
                matches.get_one::<String>("location").unwrap(),
            )?)?;

            dot_kill(&config)?;
        }
        Some(("reload", matches)) => {
            let config = get_active_config(get_dot_info_from_arg(
                matches.get_one::<String>("location").unwrap(),
            )?)?;

            dot_reload(&config)?;
        }
        Some(("run", matches)) => {
            let prog = matches.get_one("Program").unwrap();

            run(prog);
        }
        Some(("get", matches)) => {
            // ew, only temporary I hope
            dot_get(matches, folder_path)?;
        }
        Some(("profile", matches)) => match matches.subcommand() {
            Some(("set", pmatches)) => {
                let to_set = pmatches.get_one::<String>("Profile").unwrap();

                let profile = profiles
                    .iter()
                    .find(|dp| &dp.name == to_set)
                    .context("Profile doesn't exist!")?;

                // run profile on_start commands
                if let Some(start) = &profile.start {
                    for cmd in start {
                        process::Command::new("sh")
                            .args(["-c", cmd])
                            .output()
                            .context("Couldn't run command '{cmd}'")?;
                    }
                }

                // set all dots from profile
                if let Some(pdots) = &profile.dots {
                    for (df, dt) in pdots.iter() {
                        let dotfolder_path = folder_path.join(df);
                        let dot_path = dotfolder_path.join(dt);

                        let dotfolder = process_dotfolder(&dotfolder_path)?;
                        let dot = process_dot(&dot_path)?;
                        let config = get_active_config((&dotfolder, Some(&dot)))?;
                        let conf_path = Path::new(&config.destination);

                        dot_set(&config, &dot_path, conf_path)?;
                    }
                } else {
                    println!("There are no Dots specified in 'dots'!");
                }
            }
            Some(("list", _)) => {
                for dp in profiles {
                    println!("{}", dp.name);
                }
            }
            _ => unreachable!(),
        },
        _ => unreachable!(),
    }

    Ok(())
}

fn dot_set(config: &DotConfig, dot_path: &Path, conf_path: &Path) -> Result<()> {
    if !conf_path.exists() {
        if let Some(parent_path) = conf_path.parent() {
            if !parent_path.exists() {
                fs::create_dir_all(parent_path).expect("Couldn't create parent path recursively.");
            }
        }
    }

    if conf_path.is_file() {
        fs::remove_file(conf_path).expect("Couldn't remove old dot file.");
    } else if conf_path.is_dir() {
        fs::remove_dir_all(conf_path).expect("Couldn't remove the old Dot folder.");
    }

    symlink(dot_path, conf_path).expect("Couldn't create a symlink.");

    // if 'reload' exists or both 'start' and 'kill' are specified, we can reload
    // only if 'reload_on_set' is set to 'true', which is the default value.
    if config.reload.is_some() || (config.start.is_some() && config.kill.is_some()) {
        match config.reload_on_set {
            Some(x) if x => dot_reload(config)?,
            None => dot_reload(config)?,
            _ => return Ok(()),
        }
    }

    Ok(())
}

// run a program, make it a daemon, exit
fn run(prog: &String) {
    if let Ok(Fork::Child) = daemon(false, false) {
        let _ = exec::Command::new("sh").args(&["-c", prog]).exec();
    }
}

fn dot_start(config: &DotConfig) -> Result<()> {
    if let Some(start_cmd) = &config.start {
        run(start_cmd);
    } else {
        bail!("No 'start' command specified in any .dothub .")
    }

    Ok(())
}

fn dot_kill(config: &DotConfig) -> Result<()> {
    if let Some(kill_cmd) = &config.kill {
        process::Command::new("sh")
            .args(["-c", kill_cmd])
            .output()
            .context("Couldn't kill Dot.")?;
    } else {
        bail!("No 'kill' command specified in any .dothub .");
    }

    Ok(())
}

fn dot_reload(config: &DotConfig) -> Result<()> {
    if let Some(reload_cmd) = &config.reload {
        process::Command::new("sh")
            .args(["-c", reload_cmd])
            .output()
            .context("Couldn't reload Dot.")?;
    } else if let (Some(start_cmd), Some(kill_cmd)) = (&config.start, &config.kill) {
        process::Command::new("sh")
            .args(["-c", &format!("{} && {}", &kill_cmd, &start_cmd)])
            .output()
            .context("Couldn't reload Dot.")?;
    } else {
        bail!("No 'reload' command specified in any .dothub .");
    }

    Ok(())
}

fn dot_get(matches: &ArgMatches, folder_path: &Path) -> Result<()> {
    // check if all paths given are valid
    let paths = matches
        .get_many::<String>("paths")
        .unwrap()
        .filter_map(|p| {
            let path = Path::new(p);
            if !path.exists() {
                println!("'{}' doesn't exist!", p);
                None
            } else {
                Some(path)
            }
        });

    println!(
        "For every path, input it's location in your .dothub. Example: 'polybar/red_one'.
Nonexistent folders are gonna be created.
Existent 'Dots' are gonna be erased.\n"
    );

    // helper function, i am lazy, I miss Python
    let input = |msg: &str| -> String {
        println!("{msg}");

        let mut buf = "".to_string();

        io::stdout()
            .flush()
            .expect("Flush broken, try a different toilet.");
        io::stdin()
            .read_line(&mut buf)
            .expect("Reading from stdin failed.");

        buf
    };

    // for each path arg, input it's .dothub location
    paths.for_each(|p| {
        'main: loop {
            let location = input(&format!("'{}': ", p.to_str().unwrap()));

            // trim the leading '\n'
            let location = location.trim();

            // Dotfolder/Dot parsing
            if let Some((_, d)) = location.split_once('/') {
                let final_destination = folder_path.join(location);

                if d.is_empty() {
                    println!("You have to input the 'Dot'.");
                    continue;
                }

                // if the destination in .dothub already exists, ask if it should be overwritten
                if final_destination.exists() {
                    loop {
                        let user_choice = input(&format!(
                            "'{}' already exists, do you want to replace it? [y/n]: ",
                            location
                        ));

                        if let Some(choice) = user_choice.to_lowercase().chars().next() {
                            match choice {
                                'y' => {
                                    fs::remove_dir_all(&final_destination).unwrap();
                                    break;
                                }
                                'n' => {
                                    break 'main;
                                }
                                _ => {}
                            }
                        }
                    }
                } else {
                    fs::create_dir_all(&final_destination)
                        .expect("Couldn't create a new 'Dot' in your .dothub .");
                }

                if p.is_file() {
                    fs::copy(p, final_destination.join(p.file_name().unwrap()))
                        .expect("Couldn't copy dot file over to your .dothub .");
                } else {
                    let mut options = fs_extra::dir::CopyOptions::new();
                    options.content_only = true;

                    fs_extra::dir::copy(p, final_destination, &options)
                        .expect("Couldn't copy dot folder over to your .dothub .");
                }

                break;
            } else {
                println!("The format is 'DotFolder/Dot'");
            }
        }
    });

    Ok(())
}

fn process_profile(path: PathBuf) -> Result<Profile> {
    let name = path
        .with_extension("")
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let profile_contents = fs::read_to_string(&path).expect("Couldn't read profile.");

    let parsed: ProfileParsable =
        toml::from_str(&profile_contents).context("Couldn't parse a profile.")?;

    Ok(Profile {
        name,
        start: parsed.start,
        dots: parsed.dots,
    })
}

fn process_dotfolder(path: &Path) -> Result<DotFolder> {
    let name = path.file_name().unwrap().to_str().unwrap().to_owned();
    let mut config: Option<DotConfig> = None;

    let dots_paths = path.read_dir().unwrap();

    let dots: Result<Vec<Dot>, anyhow::Error> = dots_paths
        .filter_map(|dot_path| {
            let dot_path = &dot_path.expect("Couldn't read Dot.").path();
            let dot_path_name = dot_path.file_name().unwrap().to_str().unwrap();

            if dot_path.is_dir() {
                return Some(process_dot(dot_path));
            } else if dot_path.is_file() && dot_path_name == ".dothub" {
                let user_home = match env::var("HOME").context("No $HOME set!") {
                    Ok(value) => value,
                    Err(e) => return Some(Err(e)),
                };

                let config_file = fs::read_to_string(dot_path)
                    .expect("Couldn't read .dothub .")
                    .replace('~', &user_home);

                let parsed = toml::from_str(&config_file)
                    .with_context(|| format!("'{}' .dothub couldn't be parsed.", name));

                match parsed {
                    Ok(conf) => config = Some(conf),
                    Err(e) => return Some(Err(e)),
                }
            }
            None
        })
        .collect();

    match dots {
        Ok(dots) => Ok(DotFolder { name, dots, config }),
        Err(e) => Err(e),
    }
}

fn process_dot(path: &Path) -> Result<Dot> {
    let name = path.file_name().unwrap().to_str().unwrap().to_owned();
    let mut config: Option<DotConfig> = None;

    let dots_files = path.read_dir().unwrap();
    for dot_path in dots_files {
        let dot_path = dot_path.expect("Couldn't read Dot.").path();
        let dot_path_name = dot_path.file_name().unwrap().to_str().unwrap();

        if dot_path.is_file() && dot_path_name == ".dothub" {
            let user_home = env::var("HOME").context("No $HOME set!")?;

            let config_file = fs::read_to_string(dot_path)
                .expect("Couldn't read .dothub .")
                .replace('~', &user_home);

            config = Some(toml::from_str(&config_file).context("Dot .dothub couldn't be parsed")?);
        }
    }

    Ok(Dot { name, config })
}

fn arguments() -> clap::ArgMatches {
    Command::new("dothub")
        .about("Manage your dofiles from a comfortable hub!")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .author("Yours truly")
        .subcommand(
            Command::new("set")
                .about("Applies a Dot.")
                .arg(Arg::new("location")
                    .help("Dotfolder/Dot, example 'waybar/neon'.")
                    .required(true))
        )
        .subcommand(
            Command::new("watch")
                .about("Watches a Dot and reloads on a change.")
                .arg(Arg::new("location")
                    .help("Dotfolder/Dot, example 'waybar/neon'.")
                    .required(true))
        )
        .subcommand(
            Command::new("list")
                .about("Lists all Dots.")
        )
        .subcommand(
            Command::new("start")
                .about("Runs the 'start' command. DotFolder config used if Dot isn't specified, or there is no Dot config")
                .arg(Arg::new("location")
                    .help("DotFolder/<Dot>, DotFolder has to be present but Dot can be not specified. Example 'waybar', or 'waybar/neon'.")
                    .required(true))
        )
        .subcommand(
            Command::new("kill")
                .about("Runs the 'kill' command. DotFolder config used if Dot isn't specified, or there is no Dot config")
                .arg(Arg::new("location")
                    .help("DotFolder/<Dot>, DotFolder has to be present but Dot can be not specified. Example 'waybar', or 'waybar/neon'.")
                    .required(true))
        )
        .subcommand(
            Command::new("reload")
                .about("Runs the 'reload' command. DotFolder config used if Dot isn't specified, or there is no Dot config")
                .arg(Arg::new("location")
                    .help("DotFolder/<Dot>, DotFolder has to be present but Dot can be not specified. Example 'waybar', or 'waybar/neon'.")
                    .required(true))
        )
        .subcommand(
            Command::new("run")
                .about("Runs a program, forked, with a different PID.")
                .arg(Arg::new("Program").required(true))
        )
        .subcommand(
            Command::new("get")
                .about("Get your existing dotfiles into your .dothub. Input multiple relative or absolute paths, you'll give them all .dothub locations separately.")
                .arg(Arg::new("paths")
                    .help("Relative or absolute paths to your existing dot files.")
                    .required(true)
                    .num_args(0..)
                )
        )
        .subcommand(
            Command::new("profile")
                .about("Profiles")
                .subcommand_required(true)
                .arg_required_else_help(true)
                .subcommand(
                    Command::new("set")
                        .about("Sets a profile.")
                        .arg(Arg::new("Profile").required(true))
                )
                .subcommand(
                    Command::new("list")
                        .about("Lists all DotProfiles.")
                )
        )
        .get_matches()
}
