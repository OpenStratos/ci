/// OpenStratos CI.
#[macro_use]
extern crate clap;
extern crate serde;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate error_chain;
extern crate colored;
extern crate reqwest;

use std::io::{self, Write, Read};
use std::process::{Command, exit};
use std::path::PathBuf;

use clap::{Arg, App};

/// OpenStratos repository path.
const OPENSTRATOS_REPO: &str = "/opt/openstratos/server-rs";
/// OpenStratos REST API endpoint.
const OPENSTRATOS_REST: &str = "http://staging.openstratos.org/test";
/// OpenStratos REST API key length.
const KEY_LEN: usize = 20;

mod error {
    error_chain!{
        foreign_links {
            Io(::std::io::Error);
            Reqwest(::reqwest::Error);
        }

        errors {
            RequestPost(status_code: ::reqwest::StatusCode, response: String) {
                description("non-OK status code in REST API")
                display("A '{}' status code was received, with this response body:\n{}",
                        status_code,
                        response)
            }
        }
    }
}

use error::*;

/// Test results.
#[derive(Debug, Clone, Default, Serialize)]
struct TestResult {
    build: bool,
    build_stdout: String,
    build_stderr: String,
    test: bool,
    test_stdout: String,
    test_stderr: String,
    features: Vec<&'static str>,
}

fn main() {
    use colored::Colorize;

    if let Err(e) = run() {
        println!("{}{}", "An error occurred: ".red(), format!("{}", e).red());

        for e in e.iter().skip(1) {
            println!("{}", format!("\tcaused by: {}", e).red());
        }

        // The backtrace is not always generated.
        if let Some(backtrace) = e.backtrace() {
            println!();
            println!("{}", format!("\tbacktrace: {:?}", backtrace).red());
        }

        exit(1);
    } else {
        println!("{}", "All tests OK".green());
    }
}

fn run() -> Result<()> {
    let cli = cli().get_matches();

    println!("Please, insert your authentication key:");
    let mut key = String::new();
    io::stdin().read_line(&mut key)?;

    while key.trim().len() != KEY_LEN {
        println!("Invalid key, please, insert the correct key:");
        key.clear();
        io::stdin().read_line(&mut key)?;
    }
    let key = key.trim();

    let mut result = TestResult::default();
    let repo = PathBuf::from(OPENSTRATOS_REPO);
    let manifest = repo.clone().join("Cargo.toml");

    let build = Command::new("cargo")
        .arg("build")
        .arg("--manifest-path")
        .arg(&manifest)
        .output()
        .chain_err(|| "error running the build command")?;

    result.build = build.status.success();
    result.build_stdout = String::from_utf8_lossy(&build.stdout).into_owned();
    result.build_stderr = String::from_utf8_lossy(&build.stderr).into_owned();

    let mut features = Vec::new();
    if cli.is_present("raspicam") {
        features.push("raspicam");
    }
    if cli.is_present("fona") {
        features.push("fona");
    }
    if cli.is_present("no_sms") {
        features.push("no_sms");
    } else {
        print!("You decided to test by sending SMSs but this can cost you money, are you sure? \
                  (y/n)");
        io::stdout().flush()?;
        let mut response = String::new();
        io::stdin().read_line(&mut response)?;

        while response.trim() != "y" && response.trim() != "n" {
            print!("Please, select 'y' (yes) or 'n' (no)");
            io::stdout().flush()?;
            response.clear();
            io::stdin().read_line(&mut response)?;
        }

        match response.trim() {
            "y" => {}
            "n" => {
                println!("Aborting test.");
                return Ok(());
            }
            _ => unreachable!(),
        }
    }
    if cli.is_present("gps") {
        features.push("gps")
    }
    if cli.is_present("telemetry") {
        features.push("telemetry");
    }
    if cli.is_present("no_power_off") {
        features.push("no_power_off");
    }


    let features_str = {
        let mut features_iter = features.iter().cloned();
        let mut features_str: String = if let Some(feature) = features_iter.next() {
            feature.to_owned()
        } else {
            String::new()
        };
        for feature in features_iter {
            features_str.push(' ');
            features_str.push_str(feature);
        }
        features_str
    };
    result.features = features;

    let mut test = Command::new("cargo");
    test.arg("test")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--no-default-features");
    if !features_str.is_empty() {
        test.arg("--features").arg(features_str);
    }
    let test = test.arg("--")
        .arg("--ignored")
        .output()
        .chain_err(|| "error running the default features test command")?;

    result.test = test.status.success();
    result.test_stdout = String::from_utf8_lossy(&test.stdout).into_owned();
    result.test_stderr = String::from_utf8_lossy(&test.stderr).into_owned();

    send_result(key, &result).chain_err(|| "error sending result")
}

fn send_result<S: Into<String>>(key: S, result: &TestResult) -> Result<()> {
    use reqwest::{Client, StatusCode};

    let client = Client::new()?;
    let mut req = client
        .post(OPENSTRATOS_REST)
        .json(&result)
        .basic_auth(key.into(), None)
        .send()?;
    if req.status() != &StatusCode::Ok {
        let mut response = Vec::new();
        req.read_to_end(&mut response)?;
        Err(ErrorKind::RequestPost(*req.status(),
                                   String::from_utf8_lossy(&response).into_owned())
                    .into())
    } else {
        Ok(())
    }
}

fn cli() -> App<'static, 'static> {
    App::new("OpenStratos Continuous Integration")
        .version(crate_version!())
        .author("OpenStratos")
        .about("Checks OpenStratos code in the real testing probe, with real hardware.")
        .arg(Arg::with_name("raspicam")
                 .long("raspicam")
                 .help("Wether to test the Raspberry Pi camera.")
                 .takes_value(false))
        .arg(Arg::with_name("fona")
                 .long("fona")
                 .help("Wether to test the Adafruit FONA module.")
                 .takes_value(false))
        .arg(Arg::with_name("no_sms")
                 .long("no_sms")
                 .help("Do not send SMSs.")
                 .takes_value(false)
                 .requires("fona"))
        .arg(Arg::with_name("gps")
                 .long("gps")
                 .help("Wether to test the GPS module.")
                 .takes_value(false))
        .arg(Arg::with_name("telemetry")
                 .long("telemetry")
                 .help("Wether to test the telemetry module.")
                 .takes_value(false))
        .arg(Arg::with_name("no_power_off")
                 .long("no_power_off")
                 .help("Do not power the Raspberry Pi off.")
                 .takes_value(false))
}
