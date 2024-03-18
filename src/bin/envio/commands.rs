/// Implementation of all the subcommands that can be run by the CLI
use colored::Colorize;
use inquire::{min_length, Confirm, MultiSelect, Password, PasswordDisplayMode, Select, Text};
use regex::Regex;

use std::collections::HashMap;
use std::env;
use std::io::Read;
use std::path::Path;
use url::Url;

use envio::crypto::gpg::get_gpg_keys;
use envio::crypto::{create_encryption_type, get_encryption_type};

use envio::Profile;

use crate::clap_app::Command;
use crate::cli;
use crate::utils::parse_envs_from_string;

/**
 * Get the user key from the user using the inquire crate

 @return String
*/
fn get_userkey() -> String {
    println!("{}", "Loading Profile".green());
    println!("{}", "Enter your encryption key".green());
    let prompt = Password::new("Enter your encryption key:")
        .with_display_toggle_enabled()
        .with_display_mode(PasswordDisplayMode::Masked)
        .with_help_message("OH NO! you forgot your key! just kidding... or did you?")
        .without_confirmation()
        .prompt();

    if let Err(e) = prompt {
        println!("{}: {}", "Error".red(), e);
        std::process::exit(1);
    } else {
        prompt.unwrap()
    }
}

/**
 * Check to see if the user is using a vi based editor so that we can use the vim mode in the inquire crate

 @return Result<bool, String>
*/
fn get_vim_mode() -> Result<bool, String> {
    let env = env::var("VISUAL").unwrap_or_else(|_| env::var("EDITOR").unwrap_or_default());

    let program = env.split_whitespace().next().ok_or("")?; // Throw an error if the program is empty, we don't really care about the error message

    let program_stem = Path::new(program)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or("")?; // Same here

    Ok(Regex::new(r"n?vim?").unwrap().is_match(program_stem)) // unwrap is safe here because we know that the regex will always compile
}

impl Command {
    /**
     * Run the subcommand that was passed to the program
     */
    pub fn run(&self) {
        let vim_mode = get_vim_mode().unwrap_or(false);

        match self {
            Command::Create {
                profile_name,
                envs,
                envs_file,
                gpg,
            } => {
                if profile_name.is_empty() {
                    println!("{}: Profile name can not be empty!", "Error".red());
                    return;
                }

                if Profile::does_exist(profile_name) {
                    println!("{}: Profile already exists", "Error".red());
                    return;
                }

                let gpg_key;
                let encryption_type;

                if gpg.is_some() {
                    if gpg.as_ref().unwrap() == "select" {
                        let available_keys;

                        #[cfg(target_family = "unix")]
                        {
                            available_keys = get_gpg_keys();
                        }

                        #[cfg(target_family = "windows")]
                        {
                            available_keys = get_gpg_keys().unwrap();
                            if available_keys.len() == 0 {
                                println!("{}: No GPG keys found", "Error".red());
                                return;
                            }
                        }

                        let ans = Select::new(
                            "Select the GPG key you want to use for encryption:",
                            available_keys.iter().map(|(s, _)| s.clone()).collect(),
                        )
                        .with_vim_mode(vim_mode)
                        .prompt();

                        if let Err(e) = ans {
                            println!("{}: {}", "Error".red(), e);
                            return;
                        }

                        gpg_key = available_keys
                            .iter()
                            .find_map(|(k, f)| {
                                if k == ans.as_ref().unwrap() {
                                    Some(f.clone())
                                } else {
                                    None
                                }
                            })
                            .unwrap();
                    } else {
                        gpg_key = gpg.as_ref().unwrap().to_string();
                    }
                    encryption_type = create_encryption_type(gpg_key, "gpg");
                } else {
                    let prompt = Password::new("Enter your encryption key:")
                        .with_display_toggle_enabled()
                        .with_display_mode(PasswordDisplayMode::Masked)
                        .with_validator(min_length!(8))
                        .with_formatter(&|_| String::from("Input received"))
                        .with_help_message(
                            "Remeber this key, you will need it to decrypt your profile later",
                        )
                        .with_custom_confirmation_error_message("The keys don't match.")
                        .prompt();

                    let user_key = if let Err(e) = prompt {
                        println!("{}: {}", "Error".red(), e);
                        return;
                    } else {
                        prompt.unwrap()
                    };

                    encryption_type = create_encryption_type(user_key, "age");
                }

                let mut envs_hashmap;

                if envs_file.is_some() {
                    let file = envs_file.as_ref().unwrap();

                    if !Path::new(file).exists() {
                        println!("{}: File does not exist", "Error".red());
                        return;
                    }

                    let mut file = std::fs::OpenOptions::new().read(true).open(file).unwrap();

                    let mut buffer = String::new();
                    file.read_to_string(&mut buffer).unwrap();

                    envs_hashmap = Some(parse_envs_from_string(&buffer));

                    if envs_hashmap.is_none() {
                        println!("{}: Unable to parse the file", "Error".red());
                        return;
                    }

                    let mut options = vec![];

                    for (key, value) in envs_hashmap.as_ref().unwrap().clone() {
                        if value.is_empty() {
                            let prompt = Confirm::new(&format!(
                                "Would you like to assign a value to key: {} ?",
                                key
                            ))
                            .with_default(false)
                            .with_help_message(
                                "If you do not want to assign a value to this key, press enter",
                            )
                            .prompt();

                            if let Err(e) = prompt {
                                println!("{}: {}", "Error".red(), e);
                                std::process::exit(1);
                            } else if prompt.unwrap() {
                                let prompt =
                                    Text::new(&format!("Enter the value for {}:", key)).prompt();

                                if let Err(e) = prompt {
                                    println!("{}: {}", "Error".red(), e);
                                    std::process::exit(1);
                                } else {
                                    envs_hashmap
                                        .as_mut()
                                        .unwrap()
                                        .insert(key.to_string(), prompt.unwrap());
                                }
                            }
                        }

                        // we add the keys to the options list so that we can use them in the multi select prompt.
                        // The reason we do not have this in a separate loop is for efficiency reasons
                        options.push(key);
                    }

                    let default_options = (0..options.len()).collect::<Vec<usize>>();

                    let prompt = MultiSelect::new("Select the environment variables you want to keep in your new profile:", options.clone())
                        .with_default(&default_options)
                        .with_vim_mode(vim_mode)
                        .with_help_message("↑↓ to move, space to select/unselect one, → to all, ← to none, type to filter, enter to confirm")
                        .prompt();

                    if let Err(e) = prompt {
                        println!("{}: {}", "Error".red(), e);
                        std::process::exit(1);
                    } else {
                        // remove the keys that were not selected
                        let selected_keys = prompt.unwrap();

                        for key in options {
                            if !selected_keys.contains(&key) {
                                envs_hashmap.as_mut().unwrap().remove(&key);
                            }
                        }
                    }
                } else if envs.is_some() {
                    envs_hashmap = Some(HashMap::new());

                    for env in envs.as_ref().unwrap() {
                        if (*env).contains('=') {
                            let mut parts = env.splitn(2, '=');

                            if let Some(key) = parts.next() {
                                if let Some(value) = parts.next() {
                                    envs_hashmap
                                        .as_mut()
                                        .unwrap()
                                        .insert(key.to_string(), value.to_string());
                                } else {
                                    println!(
                                        "{}: Unable to parse value for key '{}'",
                                        "Error".red(),
                                        key
                                    );
                                }
                            } else {
                                println!(
                                    "{}: Unable to parse key-value pair from '{}'",
                                    "Error".red(),
                                    env
                                );
                            }

                            continue;
                        }

                        let value;

                        let prompt = Text::new(&format!("Enter the value for {}:", env)).prompt();

                        if let Err(e) = prompt {
                            println!("{}: {}", "Error".red(), e);
                            std::process::exit(1);
                        } else {
                            value = prompt.unwrap();
                            envs_hashmap
                                .as_mut()
                                .unwrap()
                                .insert(env.to_string(), value);
                        }
                    }
                } else {
                    envs_hashmap = None;
                }

                cli::create_profile(profile_name.to_string(), envs_hashmap, encryption_type);
            }

            Command::Add { profile_name, envs } => {
                if !Profile::does_exist(profile_name) {
                    println!("{}: Profile does not exist", "Error".red());
                    return;
                }

                let encryption_type = get_encryption_type(profile_name, Some(get_userkey));

                let mut profile = if let Some(p) = Profile::load(profile_name, encryption_type) {
                    p
                } else {
                    return;
                };

                for env in envs {
                    if (*env).contains('=') {
                        let mut parts = env.splitn(2, '=');

                        if let Some(key) = parts.next() {
                            if profile.envs.contains_key(key) {
                                println!(
                                    "{}: The environment variable `{}` already exists in profile",
                                    "Error".red(),
                                    key
                                );
                                return;
                            }

                            if let Some(value) = parts.next() {
                                profile.insert_env(key.to_string(), value.to_string())
                            } else {
                                println!(
                                    "{}: Unable to parse value for key '{}'",
                                    "Error".red(),
                                    key
                                );
                            }
                        } else {
                            println!(
                                "{}: Unable to parse key-value pair from '{}'",
                                "Error".red(),
                                env
                            );
                        }

                        continue;
                    }

                    if profile.envs.contains_key(env) {
                        println!(
                            "{}: The environment variable `{}` already exists in profile",
                            "Error".red(),
                            env
                        );
                        return;
                    }

                    let value;

                    let prompt = Text::new(&format!("Enter the value for {}:", env)).prompt();

                    if let Err(e) = prompt {
                        println!("{}: {}", "Error".red(), e);
                        std::process::exit(1);
                    } else {
                        value = prompt.unwrap();
                        profile.insert_env(env.to_string(), value)
                    }
                }
                println!("{}", "Applying Changes".green());
                profile.push_changes();
            }

            Command::Load { profile_name } => {
                #[cfg(target_family = "unix")]
                {
                    cli::load_profile(profile_name);
                }

                #[cfg(target_family = "windows")]
                {
                    if !Profile::does_exist(profile_name) {
                        println!("{}: Profile does not exist", "Error".red());
                        return;
                    }

                    let encryption_type = get_encryption_type(profile_name, Some(get_userkey));

                    let profile = if let Some(p) = Profile::load(profile_name, encryption_type) {
                        p
                    } else {
                        return;
                    };

                    cli::load_profile(profile);
                }
            }

            #[cfg(target_family = "unix")]
            Command::Unload => {
                cli::unload_profile();
            }

            #[cfg(target_family = "windows")]
            Command::Unload { profile_name } => {
                if !Profile::does_exist(profile_name) {
                    println!("{}: Profile does not exist", "Error".red());
                    return;
                }

                let encryption_type = get_encryption_type(profile_name, Some(get_userkey));

                let profile = if let Some(p) = Profile::load(profile_name, encryption_type) {
                    p
                } else {
                    return;
                };

                cli::unload_profile(profile);
            }
            Command::Launch {
                profile_name,
                command,
            } => {
                let split_command = command.value();
                let program = split_command[0];
                let args = &split_command[1..];

                if !Profile::does_exist(profile_name) {
                    println!("{}: Profile does not exist", "Error".red());
                    return;
                }

                let encryption_type = get_encryption_type(profile_name, Some(get_userkey));

                let profile = if let Some(p) = Profile::load(profile_name, encryption_type) {
                    p
                } else {
                    return;
                };

                let mut cmd = std::process::Command::new(program)
                    .envs(profile.envs)
                    .args(args)
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit())
                    .spawn()
                    .expect("Failed to execute command");

                let status = match cmd.wait() {
                    Ok(s) => s,
                    Err(e) => {
                        println!("{}: {}", "Error".red(), e);
                        std::process::exit(1);
                    }
                };

                match status.code() {
                    Some(code) => std::process::exit(code),
                    None => {
                        println!("{}: Child process terminated by signal", "Error".red());
                        std::process::exit(1);
                    }
                }
            }

            Command::Remove { profile_name, envs } => {
                if !Profile::does_exist(profile_name) {
                    println!("{}: Profile does not exist", "Error".red());
                    return;
                }

                if envs.is_some() && !envs.as_ref().unwrap().is_empty() {
                    let encryption_type = get_encryption_type(profile_name, Some(get_userkey));

                    let mut profile = if let Some(p) = Profile::load(profile_name, encryption_type)
                    {
                        p
                    } else {
                        return;
                    };

                    for env in envs.as_ref().unwrap() {
                        profile.remove_env(env);
                    }

                    println!("{}", "Applying Changes".green());
                    profile.push_changes();
                } else {
                    cli::delete_profile(profile_name);
                }
            }

            Command::List {
                profiles,
                profile_name,
                no_pretty_print,
            } => {
                if *profiles {
                    cli::list_profiles(*no_pretty_print);
                } else if profile_name.is_some() && !profile_name.as_ref().unwrap().is_empty() {
                    if !Profile::does_exist(profile_name.as_ref().unwrap()) {
                        println!("{}: Profile does not exist", "Error".red());
                        return;
                    }

                    let encryption_type = get_encryption_type(
                        profile_name.as_ref().unwrap().as_str(),
                        Some(get_userkey),
                    );

                    let profile = if let Some(p) =
                        Profile::load(profile_name.as_ref().unwrap(), encryption_type)
                    {
                        p
                    } else {
                        return;
                    };

                    if *no_pretty_print {
                        for (key, value) in profile.envs.iter() {
                            println!("{}={}", key, value);
                        }
                    } else {
                        cli::list_envs(&profile);
                    }
                }
            }

            Command::Update { profile_name, envs } => {
                if !Profile::does_exist(profile_name) {
                    println!("{}: Profile does not exist", "Error".red());
                    return;
                }

                let encryption_type = get_encryption_type(profile_name, Some(get_userkey));

                let mut profile = if let Some(p) = Profile::load(profile_name, encryption_type) {
                    p
                } else {
                    return;
                };

                for env in envs {
                    if (*env).contains('=') {
                        let mut parts = env.splitn(2, '=');

                        if let Some(key) = parts.next() {
                            if !profile.envs.contains_key(key) {
                                println!(
                                    "{}: The environment variable `{}` does not exist in profile use the `add` command to add the variable",
                                    "Error".red(),
                                    key
                                );
                                return;
                            }

                            if let Some(value) = parts.next() {
                                profile.edit_env(key.to_string(), value.to_string())
                            } else {
                                println!(
                                    "{}: Unable to parse value for key '{}'",
                                    "Error".red(),
                                    key
                                );
                            }
                        } else {
                            println!(
                                "{}: Unable to parse key-value pair from '{}'",
                                "Error".red(),
                                env
                            );
                        }

                        continue;
                    }

                    if !profile.envs.contains_key(env) {
                        println!(
                            "{}: The environment variable `{}` does not exist in profile use the `add` command to add the variable",
                            "Error".red(),
                            env
                        );
                        return;
                    }

                    let new_value;

                    let prompt = Text::new(&format!("Enter the new value for {}:", env)).prompt();

                    if let Err(e) = prompt {
                        println!("{}: {}", "Error".red(), e);
                        std::process::exit(1);
                    } else {
                        new_value = prompt.unwrap();
                        profile.edit_env(env.to_string(), new_value)
                    }
                }

                println!("{}", "Applying Changes".green());
                profile.push_changes();
            }

            Command::Export {
                profile_name,
                file,
                envs,
            } => {
                if !Profile::does_exist(profile_name) {
                    println!("{}: Profile does not exist", "Error".red());
                    return;
                }

                let mut file_name = ".env";

                if file.is_some() {
                    file_name = &file.as_ref().unwrap()
                }

                let encryption_type = get_encryption_type(profile_name, Some(get_userkey));

                let profile = if let Some(p) = Profile::load(profile_name, encryption_type) {
                    p
                } else {
                    return;
                };

                if envs.is_some() && envs.as_ref().unwrap().contains(&"select".to_string()) {
                    let prompt = MultiSelect::new("Select the environment variables you want to export:", profile.envs.keys().collect())
                        .with_default(&(0..profile.envs.len()).collect::<Vec<usize>>())
                        .with_vim_mode(vim_mode)
                        .with_help_message("↑↓ to move, space to select/unselect one, → to all, ← to none, type to filter, enter to confirm")
                        .prompt();

                    if let Err(e) = prompt {
                        println!("{}: {}", "Error".red(), e);
                        std::process::exit(1);
                    }

                    cli::export_envs(
                        &profile,
                        file_name,
                        &Some(
                            prompt
                                .unwrap()
                                .iter()
                                .cloned()
                                .map(|s| s.to_owned())
                                .collect(),
                        ),
                    );

                    return;
                }

                cli::export_envs(&profile, file_name, envs);
            }

            Command::Import {
                profile_name,
                file,
                url,
            } => {
                if Profile::does_exist(profile_name) {
                    println!("{}: Profile already exists", "Error".red());
                    return;
                }

                if url.is_some() && Url::parse(url.as_ref().unwrap()).is_ok() {
                    cli::download_profile(
                        url.as_ref().unwrap().to_string(),
                        profile_name.to_string(),
                    );
                    return;
                }

                if file.is_some() {
                    cli::import_profile(
                        file.as_ref().unwrap().to_string(),
                        profile_name.to_string(),
                    );
                    return;
                }

                println!("{}: You must specify a file or url", "Error".red());
            }

            Command::Version { verbose } => {
                if verbose.is_some() && verbose.unwrap() {
                    println!("{} {}", "Version".green(), env!("BUILD_VERSION"));
                    println!("{} {}", "Build Timestamp".green(), env!("BUILD_TIMESTAMP"));
                    println!("{} {}", "Author".green(), env!("CARGO_PKG_AUTHORS"));
                    println!("{} {}", "License".green(), env!("CARGO_PKG_LICENSE"));
                    println!("{} {}", "Repository".green(), env!("CARGO_PKG_REPOSITORY"));
                } else {
                    println!("{} {}", "Version".green(), env!("BUILD_VERSION"));
                }
            }
        }
    }
}
