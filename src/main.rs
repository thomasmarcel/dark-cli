extern crate clap;
extern crate humansize;
extern crate regex;
extern crate reqwest;
extern crate serde;
extern crate walkdir; // could probs replace this with std::fs

#[macro_use]
extern crate failure;

use clap::{App, Arg};
use humansize::{file_size_opts as options, FileSize};
use regex::Regex;
use reqwest::{multipart, StatusCode};
use walkdir::WalkDir;

#[derive(Debug, Fail)]
enum DarkError {
    #[fail(display = "Failure to auth: {}", _0)]
    Auth(u16),
    #[fail(display = "No files found in {}.", _0)]
    NoFilesFound(String),
    #[fail(display = "Upload failure")]
    Upload(#[cause] reqwest::Error),
    #[fail(display = "Missing argument: {}", _0)]
    MissingArgument(String),
    #[fail(display = "Missing filename. (Can't happen.)")]
    MissingFilename(),
    #[fail(display = "Regex error.")]
    Regex(),
    #[fail(display = "No SET-COOKIE header received.")]
    MissingSetCookie(),
    #[fail(display = "Unknown failure")]
    Unknown,
}

impl From<regex::Error> for DarkError {
    fn from(_err: regex::Error) -> Self {
        DarkError::Unknown
    }
}

impl From<reqwest::Error> for DarkError {
    fn from(_err: reqwest::Error) -> Self {
        DarkError::Unknown
    }
}

impl From<reqwest::header::ToStrError> for DarkError {
    fn from(_err: reqwest::header::ToStrError) -> Self {
        DarkError::Unknown
    }
}

// use of unstable library feature 'try_trait' (see issue #42327)
/*
impl From<std::option::NoneError> for DarkError {
    fn from(_err: std::option::NoneError) -> Self {
        DarkError::Unknown{}
    }
}
*/

impl From<std::io::Error> for DarkError {
    fn from(_err: std::io::Error) -> Self {
        DarkError::Unknown
    }
}

impl From<std::string::String> for DarkError {
    fn from(_err: std::string::String) -> Self {
        DarkError::Unknown
    }
}

impl From<walkdir::Error> for DarkError {
    fn from(_err: walkdir::Error) -> Self {
        DarkError::Unknown
    }
}

fn cookie_and_csrf(
    user: String,
    password: String,
    host: &str,
    canvas: &str,
) -> Result<(String, String), DarkError> {
    let requri = format!("{}/a/{}", host, canvas);
    let mut authresp = match reqwest::Client::new()
        .get(&requri)
        .basic_auth(user, Some(password))
        .send()
    {
        Ok(r) => r,
        Err(error) => panic!("Error authing: {:?}", error),
    };

    match authresp.status() {
        StatusCode::OK => (),
        _ => {
            return Err(DarkError::Auth(authresp.status().as_u16()));
        }
    }

    let cookie: String = authresp
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .ok_or(DarkError::MissingSetCookie())?
        .to_str()?
        .to_string();

    let csrf_re: Regex = Regex::new("const csrfToken = \"([^\"]*)\";")?;
    let csrf: String = csrf_re
        .captures_iter(&authresp.text()?)
        .next()
        .ok_or(DarkError::Regex())?[1]
        .to_string();

    Ok((cookie, csrf))
}

fn form_body(paths: &str) -> Result<(reqwest::multipart::Form, u64), DarkError> {
    let mut files = paths
        .split(' ')
        .map(WalkDir::new)
        .flat_map(|entry| entry.follow_links(true).into_iter())
        .filter_map(|e| e.ok())
        .filter(|entry| entry.file_type().is_file())
        .peekable();

    // "is_empty()"
    if files.peek().is_none() {
        return Err(DarkError::NoFilesFound(paths.to_string()));
    };

    let mut len = 0;

    let mut form = multipart::Form::new();
    for file in files {
        len += file.metadata()?.len();
        println!(
            "File: {}",
            file.path()
                .file_name()
                .ok_or(DarkError::MissingFilename())?
                .to_string_lossy()
        );
        let filename = file
            .path()
            .file_name()
            .ok_or(DarkError::MissingFilename())?
            .to_string_lossy()
            .to_string();
        form = form.file(filename, file.path())?;
    }

    Ok((form, len))
}

fn main() -> Result<(), DarkError> {
    let matches = App::new("dark")
        .version("0.1.0")
        .author("Ian Smith <ismith@darklang.com")
        .about("dark cli")
        .arg(
            Arg::with_name("user")
                .long("user")
                .required(true)
                .takes_value(true)
                .help("Your dark username"),
        )
        .arg(
            Arg::with_name("password")
                .long("password")
                .required(true)
                .takes_value(true)
                .requires("user")
                .help("Your dark password"),
        )
        .arg(
            Arg::with_name("canvas")
                .long("canvas")
                .required(true)
                .takes_value(true)
                .help("Your canvas"),
        )
        .arg(
            Arg::with_name("paths")
                .required(true)
                .takes_value(true)
                .help("files to upload"),
        )
        .arg(
            Arg::with_name("dry-run")
                .long("dry-run")
                .required(false)
                .takes_value(false)
                .help("Don't upload to canvas, just print request"),
        )
        .arg(
            Arg::with_name("dev")
                .long("dev")
                .required(false)
                .takes_value(false)
                .help("Run against localhost - debug only."),
        )
        .get_matches();

    let paths = matches
        .value_of("paths")
        .ok_or_else(|| DarkError::MissingArgument("paths".to_string()))?;
    let canvas = matches
        .value_of("canvas")
        .ok_or_else(|| DarkError::MissingArgument("canvas".to_string()))?;
    let user = matches
        .value_of("user")
        .ok_or_else(|| DarkError::MissingArgument("user".to_string()))?;
    let password = matches
        .value_of("password")
        .ok_or_else(|| DarkError::MissingArgument("password".to_string()))?;
    let host = if matches.is_present("dev") {
        "http://darklang.localhost:8000"
    } else {
        "https://darklang.com"
    };
    let dryrun = matches.is_present("dry-run");

    let (cookie, csrf) = cookie_and_csrf(
        user.to_string(),
        password.to_string(),
        &host.to_string(),
        &canvas.to_string(),
    )?;

    let (form, size) = form_body(&paths.to_string())?;

    println!(
        "Going to attempt to upload files totalling {}.",
        size.file_size(options::DECIMAL)?
    );

    let requri = format!("{}/api/{}/static_assets", host, canvas);
    let client = reqwest::Client::builder()
        .gzip(true)
        .timeout(None)
        .build()?;
    let req = client
        .post(&requri)
        .header("cookie", cookie)
        .header("x-csrf-token", csrf);

    if dryrun {
        println!("{:#?}", req);
        println!("{:#?}", form);
    } else {
        let mut resp = req
            .multipart(form)
            .send()
            .or_else(|error| Err(DarkError::Upload(error)))?;
        println!("{}", resp.text()?);
    }

    Ok(())
}
