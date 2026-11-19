# MusicCLI-
A Music CLI that will hopefully do more in the future

This is a music player I have been working on that I would like to
play radio and edit music with effects on the fly via some hotkeys.

For Non-technical persons
If you are viewing this after I put an application in for a job and 
would like to test it out simply download the zip and do the following steps. 

1.) Click on the zip file at the top of the three files. 
  a.) extract after downloading
  b.) open cmd by clicking the windows icon at the bottom of the screen
  c.) while that menu is open type "cmd" and either click or press enter if selected to bring up the black cmd terminal
  d.) minimize the terminal or let it sit for a second
2.) create a folder named "MusicX" in your home directory.
3.) drag and drop any .mp3, .flac, .m4a files into that folder
3.) Go back to the extracted folder. Right click it. Then click "copy as path". 
4.)  Now go back to the terminal and type 'cd' and hit crtl+ v to copy the path into your terminal. Then hit enter. It will look like this [ cd "C:\Users\graha\Downloads\music_cli_audio_eng" ] with your "C:/Users\etc..." looking different. Don't include the [ ] symbols, just the text in between them. Then type [ music_cli.exe partofsongname ]. The songname just needs to be somewhat similar to the file name of the mp3. It uses fuzzy search so try typing in "crzy" instead of "crazy train". You might get an error if you add more words or do this [ music_cli.exe the actual song name] unfortunately right now you can only put in one word and no more. But I saw the application deadline fairly late so I hope it still works for you.  
5.) Hopefully it plays!


For technical persons
I have comments throughout the code mentioning use of AI. this is to 
keep myself accountable and ensure that I learn how to create each function and it's behavior deeply enough. I try to write what I can by myself and go through documentation. Once I get really stuck I ask AI for help. Afterwards I delete everything and rebuild it again adding a few different features. If there is no comment next to the code regarding AI then I 100% own that code unless it is the audio-engine file which is mostly AI to varying degrees.  
Also. My variable names and explanations are not cleaned up. I'm aware that they need to communicate better.
The architecture is a local socket and a previous commented out tcp implementation. The reason for this is so that a user can own the terminal and still listen to music while doing whatever they need to do without needing another window running in the background. Main searches for paths within MusicX directory and returns a path from the normal and fuzzy searches. The interprocess crate (will be used for Linux and Mac later) abstracts windows NamedPipes and uses Unix Domain sockets (Linux and Mac). Main sends this chosen path off to the rodio player via the name (ln 24), stream (ln 26) and write_all (ln 66). The corresponding setup in audio-engine catches the needed path. The other thread in audio-engine.rs is a menu of HotKeys that later on will edit the sound on the fly by adding effects. You can test the menu by hitting ctrl + alt + m, then r, then x and z, using crtl + alt + m and r to turn the two subsequent menus off. Note when the keys are registered they will be not be available for typing. Just ctrl + C or shut down the program and your keys will come back.
