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
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // MAIN

    // clap args ================================================================================================================================= //
    let fuzzy = clap::Arg::new("fuzzy")
        .long("fuzzy")
        .alias("fz")
        .short('f')
        .value_parser(value_parser!(String))
        .required(false)
        .help("type in something close to what you remember");

    let normal_srch = clap::Arg::new("normal")
        .long("normal")
        .alias("norm")
        .short('n')
        .value_parser(value_parser!(String))
        .required(false)
        .help("type in a song to play. if this is not working send me an email");

    let cmd = clap::Command::new("msc").arg(fuzzy).arg(normal_srch);
    let mut result = cmd.get_matches();

    // =========================================================================================================================================== //
    let process_os = OsStr::new("audio-engine.exe");
    let process_flag = true;

    if let Ok(s) = System::new_all() {
        if process_flag == true {
            if process_flag
                == s.processes_by_name(process_os.as_ref())
                    .any(|f| f.name() == "audio-engine.exe")
            {
                ()
            } else {
                let current_exe = std::env::current_exe().unwrap();
                let project_dir = current_exe.parent().unwrap();
                let audio_engine_path = project_dir.join("audio-engine.exe");
                Command::new(audio_engine_path).spawn();
            }
        }
    }
    let name = "audio-engine".to_ns_name::<GenericNamespaced>().unwrap(); //ai function

    let mut stream = loop {
        match LocalSocketStream::connect(name.clone()) {
            Ok(connected) => break connected,
            Err(some_error) => {
                eprint!("trying to connect to audio-engine");
                sleep(Duration::from_millis(100))
            }
        }
    };

    // finding music directory
    let start = Instant::now();
    let found_music_dir = find_Music_dir(); // add an eprint for None
    let time = start.elapsed();
    println!("Search took: {:?}", time);
    println!("pulled directory {:?}", found_music_dir);
    let x = match found_music_dir {
        Some(a) => a,
        None => panic!(),
    };
    let search_dir = file_names(x.clone());

    let mut search_pathbuf = PathBuf::new();

    if result.contains_id("fuzzy") {
        let mut push_fuzzy = String::new();
        let my_fuzz_song: Option<String> = result.remove_one("fuzzy");
        let f = my_fuzz_song.unwrap();
        push_fuzzy.push_str(&f);
        let fuzzy_path = fuzzy_search(&push_fuzzy).unwrap();
        search_pathbuf.push(fuzzy_path);
    } else if result.contains_id("normal") {
        let mut push_normal = String::new();
        let my_normal_song: Option<String> = result.remove_one("normal");
        let n = my_normal_song.unwrap();
        push_normal.push_str(&n);
        let normal_path = normal_search(&push_normal, &x).unwrap();
        search_pathbuf.push(normal_path);
    }
    println!("cmd.get_matches() holds = {result:?}");

    //fuzzy test

    let send_song: String = search_pathbuf.to_str().unwrap().to_owned();

    write_all(send_song.as_bytes()).unwrap();

    //music_test(stripped_path);
    Ok(())
} //MAIN ENDS HERE

fn find_Music_dir() -> Option<PathBuf> {
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

fn file_names(directory: PathBuf) -> io::Result<Option<()>> {
    // this shows file names im working with, can delete this later
    let workon = directory;
    for entry in read_dir(workon)? {
        let entry = entry?;
    }
    Ok(Some(()))
}

fn normal_search(search_arg: &String, directory: &PathBuf) -> Result<PathBuf, std::io::Error> {
    let lower = search_arg.to_lowercase();
    let reading_dir = read_dir(directory)?;
    let mut cheating = PathBuf::new();
    let names = reading_dir
        .filter_map(|entry| {
            if entry.is_ok() {
                let entry = entry.ok()?;

                if entry
                    .file_name()
                    .into_string()
                    .ok()?
                    .to_lowercase()
                    .contains(&lower)
                {
                    cheating.push(entry.path()); // testing
                    Some(entry.path())
                } else {
                    None
                }
            } else {
                println!("shouldn't be printing");
                None
            }
        })
        .collect::<Vec<_>>();

    Ok(cheating)

}

fn fuzzy_search(search_arg: &String) -> Option<PathBuf> {
    let mut matcher = Matcher::new(Config::DEFAULT);
    let matches = Pattern::new(
        search_arg,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );
    //let song_names = vec![]; // empty vector if needed or want to use later
    let dir = find_Music_dir()?; // will have to save path and run once later instead of doing this
    let reading = dir.read_dir().ok()?;
    let next_thing = reading
        .filter_map(|dir_entry| {
            let a = dir_entry.unwrap();
            let b = a.file_name().into_string().ok()?;
            let mut blank_buffer: Vec<_> = vec![];
            let c = Utf32Str::new(&b, &mut blank_buffer);
            let score = matches.score(c, &mut matcher);
            let pair = (Some(score), a.path());
            Some(pair)
        })
        .max_by_key(|(score, _)| *score);
    Some(next_thing.unwrap().1)
}
