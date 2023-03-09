// This module provides routines for reading ripgrep config "rc" files. The
// primary output of these routines is a sequence of arguments, where each
// argument corresponds precisely to one shell argument.

use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use bstr::{io::BufReadExt, ByteSlice};
use log;

use crate::Result;

/// Return a sequence of arguments derived from ripgrep rc configuration files.
pub fn args() -> Vec<OsString> {
    let config_path = match config_path() {
        None => return vec![],
        Some(config_path) => config_path,
    };

    let (args, errs) = match parse(&config_path) {
        Ok((args, errs)) => (args, errs),
        Err(err) => {
            message!(
                "failed to read the file specified in RIPGREP_CONFIG_PATH: {}",
                err
            );
            return vec![];
        }
    };

    if !errs.is_empty() {
        for err in errs {
            message!("{}:{}", config_path.display(), err);
        }
    }
    log::debug!(
        "{}: arguments loaded from config file: {:?}",
        config_path.display(),
        args
    );
    args
}

/// returns the path of a config file in this precedence
/// 1) cwd
/// 2) env specified
/// 3) somewhere up the tree from cwd
fn config_path() -> Option<PathBuf> {
    let cwd_opt = cwd_ripgreprc();
    if cwd_opt.is_some() {
        return cwd_opt;
    }

    let env_opt = env_ripgreprc();
    if env_opt.is_some()  {
        return env_opt;
    }

    return find_ripgreprc();
}

/// if there is a ripgreprc in the cwd, get it
fn cwd_ripgreprc() -> Option<PathBuf> {
    let mut cwd = env::current_dir().unwrap();
    let file = Path::new(".ripgreprc");

    cwd.push(file);

    if cwd.is_file() {
        return Some(cwd);
    }

    None
}

/// if we have a ripgreprc specified in env, get it
fn env_ripgreprc() -> Option<PathBuf> { 
    match env::var_os("RIPGREP_CONFIG_PATH") {
        None => None,
        Some(config_path) => {
            if config_path.is_empty() {
                return None;
            } else {
                return Some(PathBuf::from(config_path));
            }
        }
    }
}

/// Find a .ripgreprc file in the tree
fn find_ripgreprc() -> Option<PathBuf> {
    let mut search_path = env::current_dir().unwrap();
    let file = Path::new(".ripgreprc");

    // go up one, since we know it's not in the current folder already
    if !search_path.pop() {
        return None;
    }

    loop {
        search_path.push(file);

        if search_path.is_file() {
            break Some(search_path);
        }

        if !(search_path.pop() && search_path.pop()) {
            break None;
        }
    }
}

/// Parse a single ripgrep rc file from the given path.
///
/// On success, this returns a set of shell arguments, in order, that should
/// be pre-pended to the arguments given to ripgrep at the command line.
///
/// If the file could not be read, then an error is returned. If there was
/// a problem parsing one or more lines in the file, then errors are returned
/// for each line in addition to successfully parsed arguments.
fn parse<P: AsRef<Path>>(
    path: P,
) -> Result<(Vec<OsString>, Vec<Box<dyn Error>>)> {
    let path = path.as_ref();
    match File::open(&path) {
        Ok(file) => parse_reader(file),
        Err(err) => Err(From::from(format!("{}: {}", path.display(), err))),
    }
}

/// Parse a single ripgrep rc file from the given reader.
///
/// Callers should not provided a buffered reader, as this routine will use its
/// own buffer internally.
///
/// On success, this returns a set of shell arguments, in order, that should
/// be pre-pended to the arguments given to ripgrep at the command line.
///
/// If the reader could not be read, then an error is returned. If there was a
/// problem parsing one or more lines, then errors are returned for each line
/// in addition to successfully parsed arguments.
fn parse_reader<R: io::Read>(
    rdr: R,
) -> Result<(Vec<OsString>, Vec<Box<dyn Error>>)> {
    let mut bufrdr = io::BufReader::new(rdr);
    let (mut args, mut errs) = (vec![], vec![]);
    let mut line_number = 0;
    bufrdr.for_byte_line_with_terminator(|line| {
        line_number += 1;

        let line = line.trim();
        if line.is_empty() || line[0] == b'#' {
            return Ok(true);
        }
        match line.to_os_str() {
            Ok(osstr) => {
                args.push(osstr.to_os_string());
            }
            Err(err) => {
                errs.push(format!("{}: {}", line_number, err).into());
            }
        }
        Ok(true)
    })?;
    Ok((args, errs))
}

#[cfg(test)]
mod tests {
    use super::parse_reader;
    use std::ffi::OsString;

    #[test]
    fn basic() {
        let (args, errs) = parse_reader(
            &b"\
# Test
--context=0
   --smart-case
-u


   # --bar
--foo
"[..],
        )
        .unwrap();
        assert!(errs.is_empty());
        let args: Vec<String> =
            args.into_iter().map(|s| s.into_string().unwrap()).collect();
        assert_eq!(args, vec!["--context=0", "--smart-case", "-u", "--foo",]);
    }

    // We test that we can handle invalid UTF-8 on Unix-like systems.
    #[test]
    #[cfg(unix)]
    fn error() {
        use std::os::unix::ffi::OsStringExt;

        let (args, errs) = parse_reader(
            &b"\
quux
foo\xFFbar
baz
"[..],
        )
        .unwrap();
        assert!(errs.is_empty());
        assert_eq!(
            args,
            vec![
                OsString::from("quux"),
                OsString::from_vec(b"foo\xFFbar".to_vec()),
                OsString::from("baz"),
            ]
        );
    }

    // ... but test that invalid UTF-8 fails on Windows.
    #[test]
    #[cfg(not(unix))]
    fn error() {
        let (args, errs) = parse_reader(
            &b"\
quux
foo\xFFbar
baz
"[..],
        )
        .unwrap();
        assert_eq!(errs.len(), 1);
        assert_eq!(args, vec![OsString::from("quux"), OsString::from("baz"),]);
    }
}
