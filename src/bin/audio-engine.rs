use crossterm::event::KeyEvent;
use freeverb::Freeverb;
use global_hotkey::{GlobalHotKeyEvent, hotkey::Code::ShiftLeft};
#[cfg(windows)]
use global_hotkey::{
    GlobalHotKeyManager,
    hotkey::{Code, HotKey, Modifiers},
};
use interprocess::local_socket::{
    GenericNamespaced, ListenerNonblockingMode::Stream, ListenerOptions, prelude::*,
    traits::Stream as interStream,
};
use rodio::DeviceSinkBuilder;
use rodio::{Decoder, Source, buffer::SamplesBuffer};
use rubberband::Options;
use rubberband::Stretcher;
use std::rc;
use std::{
    fs::File,
    sync::mpsc::{Receiver, Sender, sync_channel},
    vec,
};
use std::{
    fs::read_dir,
    io::{self},
    num::NonZero,
    path::PathBuf,
    time::Duration,
};
use std::{
    io::{BufRead, BufReader, Write},
    iter::zip,
    sync::{
        atomic::{
            AtomicBool, AtomicU64,
            Ordering::{Acquire, Relaxed},
        },
        mpsc::TryRecvError,
    },
};
use std::{
    io::{Error, ErrorKind, Read},
    thread::sleep,
};
use std::{num::NonZeroU16, ops::Index};
use std::{
    num::NonZeroU32,
    sync::{Arc, Mutex},
};
use std::{sync::mpsc, thread};
use windows::Win32::NetworkManagement::Ndis::OID_FDDI_PATH_RING_LATENCY;
use {interprocess::local_socket::prelude::*, std::io::prelude::*};

use crate::Effects::{Reverb, Speed};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Effects {
    Reverb,
    Pitch,
    Speed, // this needs to stay because it is placeholding the next_effect that we are not using right now. it's otherwise useless
    PlayPause,
    NoEffect,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum EffectsUpDown {
    Up,
    Down,
}
#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord)]
struct EffectChange {
    effect: Menu,
    direction: EffectsUpDown,
}
#[derive(Debug, PartialEq, Eq, Clone, Copy, PartialOrd, Ord)]
enum Menu {
    Closed,
    Main,
    Reverb,
    Pitch,
    Speed,
}
#[derive(Debug, PartialEq, Eq, Clone, Copy, PartialOrd, Ord)]
enum NextPrevious {
    Next,
    Previous,
    NoChange,
}

struct BlockFeed {
    receiver: Receiver<Vec<f32>>,
    chunk_iter: vec::IntoIter<f32>,
}

impl Iterator for BlockFeed {
    type Item = f32;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(sample) = self.chunk_iter.next() {
                return Some(sample);
            }
            self.chunk_iter = self.receiver.recv().ok()?.into_iter();
        }
    }
}

impl Source for BlockFeed {
    fn channels(&self) -> rodio::ChannelCount {
        NonZeroU16::new(2).unwrap()
    }
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        NonZeroU32::new(44100).unwrap()
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

static SPEEDHOLDER: Mutex<f64> = Mutex::new(1.0);
static PITCHHOLDER: Mutex<f64> = Mutex::new(1.0);
static REVERBHOLDER: Mutex<[f32; 4]> = Mutex::new([0.0, 1.0, 0.0, 0.0]);
static GENERATION: AtomicU64 = AtomicU64::new(0);
static PLAYPAUSE: AtomicBool = AtomicBool::new(false);

pub const SPEED: [f64; 11] = [
    1.50, 1.40, 1.30, 1.20, 1.10, 1.00, 0.90, 0.80, 0.70, 0.60, 0.50,
];

pub const PITCH: [f64; 11] = [
    0.50, 0.60, 0.70, 0.80, 0.90, 1.00, 1.10, 1.20, 1.30, 1.40, 1.50,
];

pub const REVERB_WET: [f32; 11] = [
    0.00, 0.05, 0.10, 0.15, 0.20, 0.25, 0.30, 0.35, 0.40, 0.45, 0.50,
];

pub const REVERB_DRY: [f32; 11] = [
    0.50, 0.55, 0.60, 0.65, 0.70, 0.75, 0.80, 0.85, 0.90, 0.95, 1.00,
];

pub const REVERB_ROOM_SIZE: [f32; 11] = [
    0.00, 0.10, 0.20, 0.30, 0.40, 0.50, 0.60, 0.70, 0.80, 0.90, 1.00,
];

pub const REVERB_DAMPENING: [f32; 11] = [
    0.00, 0.10, 0.20, 0.30, 0.40, 0.50, 0.60, 0.70, 0.80, 0.90, 1.00,
];

fn main() {
    println!("hi from main in audio engine via println!");

    let mut default_current_speed_idx = SPEED.iter().position(|x| *x == 1.0);
    let mut default_current_pitch_idx = PITCH.iter().position(|x| *x == 1.0);
    let mut default_current_reverb_idx = REVERB_WET.iter().position(|x| *x == 0.0);
    let (tx, rx) = mpsc::channel::<EffectChange>();
    let (send_nxt_prv, receive_nxt_prv) = mpsc::channel::<NextPrevious>();
    let mut b = NextPrevious::NoChange;
    if let Ok(adj_song) = receive_nxt_prv.try_recv() {
        b = adj_song;
        println!("the value of b from receive_nxt_prv.try_recv() = {b:?}");
    }

    //=======================================================================================================================================================//
    // Effects thread
    let read_effects = std::thread::spawn(move || {
        while let Ok(incoming_effct_chng) = rx.recv() {
            let recvd_effct_chng: EffectChange = incoming_effct_chng;

            if recvd_effct_chng.direction == EffectsUpDown::Up
                && default_current_speed_idx != Some(10)
                && recvd_effct_chng.effect == Menu::Speed
            {
                let new_speed_idx = default_current_speed_idx.unwrap() + 1;
                let new_speed = SPEED[new_speed_idx];
                default_current_speed_idx = Some(new_speed_idx);
                println!("{default_current_speed_idx:?}");
                let mut v = SPEEDHOLDER.lock().unwrap();

                *v = new_speed;
                println!("the value of v is now {:?}", v);
                drop(v);
            } else if recvd_effct_chng.direction == EffectsUpDown::Down
                && default_current_speed_idx != Some(0)
                && recvd_effct_chng.effect == Menu::Speed
            {
                let new_speed_idx = default_current_speed_idx.unwrap() - 1;
                let new_speed = SPEED[new_speed_idx];
                default_current_speed_idx = Some(new_speed_idx);
                println!("{default_current_speed_idx:?}");
                let mut v = SPEEDHOLDER.lock().unwrap();
                *v = new_speed;
                println!("the value of v is now {v:?}");
                drop(v);
            } else if recvd_effct_chng.direction == EffectsUpDown::Up
                && default_current_pitch_idx != Some(10)
                && recvd_effct_chng.effect == Menu::Pitch
            {
                let new_pitch_idx = default_current_pitch_idx.unwrap() + 1;
                let new_pitch = PITCH[new_pitch_idx];
                default_current_pitch_idx = Some(new_pitch_idx);
                let mut v = PITCHHOLDER.lock().unwrap();
                *v = new_pitch;
                println!("the value of pitch is now {v:?}");
                drop(v);
            } else if recvd_effct_chng.direction == EffectsUpDown::Down
                && default_current_pitch_idx != Some(0)
                && recvd_effct_chng.effect == Menu::Pitch
            {
                let new_pitch_idx = default_current_pitch_idx.unwrap() - 1;
                let new_pitch = PITCH[new_pitch_idx];
                default_current_pitch_idx = Some(new_pitch_idx);
                let mut v = PITCHHOLDER.lock().unwrap();
                *v = new_pitch;
                println!("the value of pitch is now {v:?}");
                drop(v);
            } else if recvd_effct_chng.direction == EffectsUpDown::Up
                && default_current_reverb_idx != Some(10)
                && recvd_effct_chng.effect == Menu::Reverb
            {
                let new_reverb_idx = default_current_reverb_idx.unwrap() + 1;
                let new_wet = REVERB_WET[new_reverb_idx];
                let new_dry = REVERB_DRY[new_reverb_idx];
                let new_room = REVERB_ROOM_SIZE[new_reverb_idx];
                let new_damp = REVERB_DAMPENING[new_reverb_idx];
                default_current_reverb_idx = Some(new_reverb_idx);
                let mut v = REVERBHOLDER.lock().unwrap();
                *v = [new_wet, new_dry, new_room, new_damp];
                drop(v);
            } else if recvd_effct_chng.direction == EffectsUpDown::Down
                && default_current_reverb_idx != Some(0)
                && recvd_effct_chng.effect == Menu::Reverb
            {
                let new_reverb_idx = default_current_reverb_idx.unwrap() - 1;
                let new_wet = REVERB_WET[new_reverb_idx];
                let new_dry = REVERB_DRY[new_reverb_idx];
                let new_room = REVERB_ROOM_SIZE[new_reverb_idx];
                let new_damp = REVERB_DAMPENING[new_reverb_idx];
                default_current_reverb_idx = Some(new_reverb_idx);
                let mut v = REVERBHOLDER.lock().unwrap();
                *v = [new_wet, new_dry, new_room, new_damp];
                drop(v);
            }
        }
    });
    // End of Effects thread
    //
    //=======================================================================================================================================================//
    //
    // Start of HotKey thread
    #[cfg(windows)]
    std::thread::spawn(move || {
        use std::collections::HashSet;

        use windows::Win32::UI::WindowsAndMessaging::GetMessageW;

        let manager = GlobalHotKeyManager::new().unwrap();
        let receiver = GlobalHotKeyEvent::receiver();
        let pause_play = HotKey::new(Some(Modifiers::CONTROL), Code::Space);
        let back = HotKey::new(Some(Modifiers::CONTROL), Code::ArrowLeft);
        let forward = HotKey::new(Some(Modifiers::CONTROL), Code::ArrowRight);
        let main_menu_keys = HotKey::new(Some(Modifiers::SHIFT | Modifiers::CONTROL), Code::KeyM);
        let reverb_menu_key = HotKey::new(None, Code::KeyR);
        let pitch_menu_key = HotKey::new(None, Code::KeyP);
        let speed_menu_key = HotKey::new(None, Code::KeyS);
        let up_key = HotKey::new(None, Code::KeyX);
        let down_key = HotKey::new(None, Code::KeyZ);

        manager.register(main_menu_keys).unwrap();
        manager.register(pause_play).unwrap();
        manager.register(back).unwrap();
        manager.register(forward).unwrap();
        let mut active_effect = Effects::NoEffect;
        let mut registered_keys: HashSet<u32> = HashSet::new();
        let mut menu = Menu::Closed;
        let keys_to_cycle = [
            reverb_menu_key,
            pitch_menu_key,
            speed_menu_key,
            up_key,
            down_key,
        ];
        let effect_keys = [reverb_menu_key, pitch_menu_key, speed_menu_key];
        let mut togglee = false;
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::MSG;
            let mut windows_message = MSG::default();

            while GetMessageW(&mut windows_message, None, 0, 0).0 > 0 {
                use windows::Win32::UI::WindowsAndMessaging::DispatchMessageW;

                DispatchMessageW(&windows_message);

                while let Ok(event) = receiver.try_recv() {
                    use global_hotkey::HotKeyState;

                    if event.state != HotKeyState::Pressed {
                        continue;
                    }
                    let (next_menu, next_effect) = if event.id == main_menu_keys.id() {
                        if menu == Menu::Closed {
                            (Some(Menu::Main), Effects::NoEffect)
                        } else {
                            (Some(Menu::Closed), Effects::NoEffect)
                        }
                    } else if menu == Menu::Main && event.id == reverb_menu_key.id() {
                        (Some(Menu::Reverb), Effects::Reverb)
                    } else if menu == Menu::Main && event.id == pitch_menu_key.id() {
                        (Some(Menu::Pitch), Effects::Pitch)
                    } else if menu == Menu::Main && event.id == speed_menu_key.id() {
                        (Some(Menu::Speed), Effects::Speed)
                    } else if menu == Menu::Reverb && event.id == reverb_menu_key.id() {
                        (Some(Menu::Main), Effects::NoEffect)
                    } else if menu == Menu::Pitch && event.id == pitch_menu_key.id() {
                        (Some(Menu::Main), Effects::NoEffect)
                    } else if menu == Menu::Speed && event.id == speed_menu_key.id() {
                        (Some(Menu::Main), Effects::NoEffect)
                    } else if menu == Menu::Reverb && event.id == main_menu_keys.id() {
                        (Some(Menu::Main), Effects::NoEffect)
                    } else if menu == Menu::Pitch && event.id == main_menu_keys.id() {
                        (Some(Menu::Main), Effects::NoEffect)
                    } else if menu == Menu::Speed && event.id == main_menu_keys.id() {
                        (Some(Menu::Main), Effects::NoEffect)
                    } else {
                        (None, Effects::NoEffect)
                    };
                    active_effect = next_effect;
                    if let Some(target) = next_menu {
                        let desired_keys = match target {
                            Menu::Closed => vec![],
                            Menu::Main => effect_keys.to_vec(),
                            Menu::Reverb => [reverb_menu_key, up_key, down_key].to_vec(),
                            Menu::Pitch => [pitch_menu_key, up_key, down_key].to_vec(),
                            Menu::Speed => [speed_menu_key, up_key, down_key].to_vec(),
                        };
                        let mut failed = false;
                        for key in keys_to_cycle {
                            if registered_keys.contains(&key.id())
                                && !desired_keys.iter().any(|k| k.id() == key.id())
                            {
                                match manager.unregister(key) {
                                    Ok(()) => {
                                        registered_keys.remove(&key.id());
                                    }
                                    Err(e) => {
                                        eprint!(
                                            "error unregistering keys or removing from registered_keys variable"
                                        );
                                        failed = true;
                                    }
                                }
                            }
                        }
                        for key in keys_to_cycle {
                            if !registered_keys.contains(&key.id())
                                && desired_keys.iter().any(|k| k.id() == key.id())
                            {
                                match manager.register(key) {
                                    Ok(()) => {
                                        registered_keys.insert(key.id());
                                    }
                                    Err(e) => {
                                        eprint!(
                                            "Error registering keys or inserting to registered_keys variable"
                                        );
                                        failed = true;
                                    }
                                }
                            }
                        }
                        if failed {
                            eprint!(
                                "did the previous errors print? if not here is desired_keys={:?}, registered_keys={:?} and failed={:?}",
                                desired_keys, registered_keys, failed
                            );
                        } else {
                            menu = target;

                            println!("{:?}", menu);
                            println!("active effect is {active_effect:?}");
                        }
                    }
                    // end of next_menu first block
                    else if matches!(menu, Menu::Reverb | Menu::Pitch | Menu::Speed) {
                        active_effect = next_effect;
                        if event.id == up_key.id() {
                            tx.send(EffectChange {
                                effect: menu, // switch this back to active_effect if you need to reset changes
                                direction: EffectsUpDown::Up,
                            });
                            println!("Up");
                        }
                        if event.id == down_key.id() {
                            println!("Down");
                            tx.send(EffectChange {
                                effect: menu, // switch this back to active_effect if you need to reset changes
                                direction: EffectsUpDown::Down,
                            });
                        }
                    } // end of matches!() block

                    if event.id == pause_play.id() {
                        togglee = !togglee;
                        let toggle = PLAYPAUSE.store(togglee, Relaxed);
                    }
                    if event.id == back.id() {
                        send_nxt_prv.send(NextPrevious::Previous);
                        println!("we are sending backwards");
                    }
                    if event.id == forward.id() {
                        send_nxt_prv.send(NextPrevious::Next);
                        println!("we are sending fowards");
                    }
                } // the end of the while let message loop
            } // end of while let loop
        } // end of unafe block
    });
    // End of HotKey thread
    //====================================================================================================================================================//
    let name = "audio-engine".to_ns_name::<GenericNamespaced>().unwrap();
    let ipc_listener = match ListenerOptions::new().name(name).create_sync() {
        Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
            eprint!("listener not working");
            return;
        }
        x => x.unwrap(),
    };

    for mut ipc_connection in ipc_listener.incoming() {
        let mut string_slate = String::with_capacity(200);
        let mut song_buf = PathBuf::new();

        let mut conn_result = ipc_connection.unwrap();
        conn_result.read_to_string(&mut string_slate);
        println!("string_slate holds {string_slate:?}");
        song_buf.push(&string_slate.to_owned());
        println!("song_buf holds {song_buf:?}");
        if song_buf == string_slate {
            let music_pass = song_buf.to_owned();
            let (send_song, receive_song): (Sender<PathBuf>, Receiver<PathBuf>) = mpsc::channel();

            let generation = GENERATION.fetch_add(1, Relaxed) + 1;
            send_song.send(music_pass);

            let song_adj_path = PathBuf::new();

            let music_controller = std::thread::spawn(move || {
                while let Ok(song_in_controller) = receive_song.try_recv() {
                    println!("before music_test() inside music_controller thread");

                    let hopefully_plays = music_test(song_in_controller, generation); // hanging here at music_test()
                    thread::sleep(Duration::from_secs(2));

                    println!("after music_test() inside music_controller thread");

                    let mut currently_playing_song = hopefully_plays.0.unwrap();
                    let music_directory = music_cli::find_Music_dir().unwrap();
                    let mut current_position = music_directory
                        .iter()
                        .position(|x| x == &currently_playing_song);
                    let mut songs = vec![];
                    let c = music_directory.to_owned();
                    for entry in c.read_dir().unwrap() {
                        let entry = entry.unwrap();
                        songs.push(entry.path());
                    }
                    if b == NextPrevious::Next && current_position.unwrap() < songs.len() {
                        current_position.unwrap() + 1;
                        println!("we are reaching the b == NextPrevious up block");
                    } else if b == NextPrevious::Previous && current_position.unwrap() > 0 {
                        current_position.unwrap() - 1;
                        println!("we are reaching the b == NextPrevious downward block");
                    }

                    let mut changed_song = songs[current_position.unwrap()].clone();
                    let send_adj_song = changed_song.to_str().unwrap().to_owned();

                    //send_song.send(changed_song);

                    ipc_connection
                        .unwrap()
                        .write(send_adj_song.as_bytes())
                        .unwrap();
                } // end of while let try_recv loop
            }); // end of music_controller thread
        } // end of if song_buf == string_slate
    } // end of conn listener IPC server loop
} // MAIN ENDS

fn music_test(song_location: PathBuf, generation: u64) -> (Option<PathBuf>, io::Result<()>) {
    println!("Entered Music Test");

    let custom_error = Error::from(ErrorKind::UnexpectedEof);
    let music_dir = song_location.parent().ok_or(custom_error).unwrap();
    let mut all_songs: Vec<PathBuf> = read_dir(music_dir)
        .unwrap()
        .filter_map(|f| {
            let path = f.ok()?.path();
            Some(path)
        })
        .collect();
    let start = all_songs
        .iter()
        .position(|p| p == &song_location)
        .unwrap_or(0); //AI
    let send = all_songs.split_off(start);
    let mut current_song = PathBuf::new();
    for sonnit in send {
        if generation != GENERATION.load(Relaxed) {
            return (None, Ok(()));
        }
        // Beginning of for loop for sonnit in send
        current_song.clear();
        current_song.push(&sonnit);

        let fle = File::open(sonnit).unwrap();
        let len = fle.metadata().unwrap().len();

        let mut decoded = Decoder::builder()
            .with_data(fle)
            .with_byte_len(len) // Enable seeking and duration calculation
            .with_seekable(true) // Enable seeking operations
            .with_gapless(false) // Enable gapless playback
            .build()
            .unwrap();

        let (sender, receiver) = sync_channel::<Vec<f32>>(4);
        let playback_thread = std::thread::spawn(move || {
            let mut handle = rodio::DeviceSinkBuilder::open_default_sink()
                .expect("make sure to handle this error later");
            handle.log_on_drop(false);

            let player = rodio::Player::connect_new(&handle.mixer());
            player.append(BlockFeed {
                receiver,
                chunk_iter: Vec::new().into_iter(),
            });

            loop {
                let flag = PLAYPAUSE.load(Relaxed);
                if flag == false {
                    player.play();
                } else {
                    player.pause();
                }
                if player.empty() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }); // end of playback thread

        let processor = std::thread::spawn(move || {
            let mut stretcher = Stretcher::new(
                44100,
                2,
                Options::PROCESS_REALTIME
                    | Options::FORMANT_PRESERVED
                    | Options::CHANNELS_TOGETHER
                    | Options::ENGINE_FASTER
                    | Options::PITCH_HIGH_CONSISTENCY
                    | Options::WINDOW_SHORT,
                1.0,
                1.0,
            );
            stretcher.set_debug_level(rubberband::DebugLevel::None);
            let mut reverb = Freeverb::<f32>::new(44100);

            loop {
                if generation != GENERATION.load(Relaxed) {
                    break;
                }
                let speed = SPEEDHOLDER.lock().expect("it's SPEEDHOLDER error");
                let pass_speed = *speed;
                drop(speed);
                let pitch = PITCHHOLDER.lock().expect("PITCHHOLDER error");
                let pass_pitch = *pitch;
                drop(pitch);
                //println!("value of pass is {:?}", pass);
                let rev = REVERBHOLDER.lock().expect("REVERBHOLDER wet error");
                let pass_wet = rev[0];
                let pass_dry = rev[1];
                let pass_room = rev[2];
                let pass_damp = rev[3];
                drop(rev);

                stretcher.set_time_ratio(pass_speed);
                stretcher.set_pitch_scale(pass_pitch);
                reverb.set_wet(pass_wet);
                reverb.set_dry(pass_dry);
                reverb.set_room_size(pass_room);
                reverb.set_dampening(pass_damp);
                let chunk: Vec<f32> = decoded.by_ref().take(2048).collect();
                let left_in: Vec<f32> = chunk.iter().step_by(2).copied().collect();
                let right_in: Vec<f32> = chunk.iter().skip(1).step_by(2).copied().collect();
                let final_chk = chunk.len() < 2048;
                stretcher.process(&[&left_in, &right_in], final_chk);
                if chunk.is_empty() {
                    break;
                };
                while let Some(available) = stretcher.available() {
                    if available == 0 {
                        break;
                    }

                    let mut left_out: Vec<f32> = vec![0.0f32; available as usize];
                    let mut right_out: Vec<f32> = vec![0.0f32; available as usize];
                    let retrieve = stretcher
                        .retrieve(&mut [left_out.as_mut_slice(), right_out.as_mut_slice()])
                        as usize;
                    let mut interleave: Vec<f32> = Vec::with_capacity(retrieve * 2);
                    let before_r = &left_out.clone();

                    let mut send_rv: Vec<(f32, f32)> =
                        left_out.into_iter().zip(right_out.into_iter()).collect();
                    // intercept reverb here

                    let oksendrvb: Vec<(f32, f32)> = send_rv
                        .into_iter()
                        .map(|(a, b)| reverb.tick((a, b)))
                        .collect();
                    let (left_r, right_r): (Vec<f32>, Vec<f32>) = oksendrvb.into_iter().unzip();
                    for i in 0..retrieve {
                        interleave.push(left_r[i]);
                        interleave.push(right_r[i]);
                    }

                    sender.send(interleave);
                } // end of while let loop
            } // end of loop
        }); // end of process_thread
        processor.join();
        //playback_thread.join();
    } // end of for sonnit in send

    (Some(current_song), Ok(()))
} // end of fn music_test
