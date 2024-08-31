use std::{
	fs::File,
	sync::mpsc::{sync_channel, RecvTimeoutError, SyncSender},
	time::Duration,
};

use raplay::{
	source::{SineSource, Symph},
	Sink,
};

fn main() {
	let (tx, rx) = sync_channel::<Msg>(8); //gen- idk why 8

	let mut sink = Sink::default();
	sink.volume(0.8).unwrap();
	sink.on_callback(Some(move |_cb_info| over(&tx))).unwrap();
	sink.on_err_callback(Some(cb_error)).unwrap();

	println!("volume = {:?}", sink.get_volume());

	let file = File::open("test.mp3").unwrap();
	let src = Symph::try_new(file, &Default::default()).unwrap();
	sink.load(src, true).unwrap();
	sink.play(true).unwrap();

	let info = sink.get_info();
	println!("{}hz // {}", info.sample_rate, info.sample_format);

	loop {
		match rx.recv_timeout(Duration::from_secs(1)) {
			Ok(msg) => {
				println!("Finished!");
				break;
			}
			Err(RecvTimeoutError::Disconnected) => {
				eprintln!("Error: channel disconnected");
				break;
			}
			Err(RecvTimeoutError::Timeout) => {
				println!("playing: {}", sink.is_playing().unwrap());
				continue;
				match sink.get_timestamp() {
					Ok(ts) => {
						println!(
							"{:.2} / {:.2}",
							ts.current.as_secs_f32(),
							ts.total.as_secs_f32()
						);
					}
					Err(e) => {
						eprintln!("Failed to get timestamp: {e}");
						break;
					}
				};
			}
		}
	}
}

enum Msg {
	Over,
}

fn over(tx: &SyncSender<Msg>) {
	if let Err(e) = tx.send(Msg::Over) {
		eprintln!("Failed to send over: {e}");
	}
}

fn cb_error(e: raplay::Error) {
	eprintln!("playback error: {e}");
}
