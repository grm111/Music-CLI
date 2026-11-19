use clap::builder::StringValueParser;
use clap::{Arg, ArgAction, command, value_parser};
use dirs::home_dir;
use interprocess::local_socket::traits::Stream;
use interprocess::local_socket::{GenericNamespaced, prelude::*};
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use std::ffi::OsStr;
use std::fmt::Error;
use std::io::Write;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;
use std::{fs::read_dir, io, path::PathBuf, time::Instant};
use sysinfo::Signal::Continue;
use sysinfo::{self, Process, System};
use walkdir::WalkDir;

pub fn find_Music_dir() -> Option<PathBuf> {
    let top_dir = home_dir()?;
    let walk = WalkDir::new(top_dir).max_depth(2); // test speed
    let waiting: Vec<_> = vec![];
    for entry in walk {
        let entry = entry.ok()?;
        if entry.path().ends_with("MusicX") {
            let path = entry.path();
            let last = path.to_path_buf();
            return Some(last);
            waiting.push(last);
            break;
        }
    }
    let strip = waiting[0].to_owned().to_path_buf();
    Some(strip)
}

pub fn next_previous_track() -> () {
    // pass the vector of songs to this function and then just move up and down and then return that pathbuf
    // cut out the finding the dir and songs from here when possible
    let music_list = find_Music_dir().unwrap();
    let gather_songs = music_list.read_dir().unwrap();
    let mut all_songs = vec![];
    for song in gather_songs {
        all_songs.push(song.unwrap().path());
    }
}
