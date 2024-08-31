use std::{
	ffi::OsStr,
	fs::File,
	path::PathBuf,
	sync::{
		atomic::{AtomicBool, Ordering},
		mpsc::{sync_channel, Receiver, SyncSender, TryRecvError},
		Arc,
	},
	thread::JoinHandle,
	time::Duration,
};

use eframe::{egui, NativeOptions};
use egui_extras::{Column, TableBuilder};
use raplay::{source::Symph, CallbackInfo, Sink, Timestamp};

fn main() {
	let nopt = NativeOptions::default();
	eframe::run_native(
		"egui app",
		nopt,
		Box::new(|cc| Ok(Box::new(GensMusic::new(cc)))),
	)
	.unwrap();
}

pub enum GenMsg {
	MediaEnded,
	/// When raplay pauses it will send an event when the buffer is empty
	/// so we can stop the loop that feeds data (i think?).
	FinishPause,
	TimestampWakeup,
}

#[derive(Debug, PartialEq)]
pub enum GenState {
	Stopped,
	Paused,
	Playing,
}

struct GensMusic {
	files: Vec<PathBuf>,

	/// Communication from other threads
	tx: SyncSender<GenMsg>,
	rx: Receiver<GenMsg>,

	// === Graphical ==
	ctx: egui::Context,

	/// Sound output and control via raplay
	sink: Sink,
	state: GenState,
	next: Option<PathBuf>,
	timekeeper: Option<JoinHandle<()>>,
	timekeeper_cancel: Arc<AtomicBool>,
}

impl GensMusic {
	pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
		// Gather all MP3 files in the current diretory
		let mut mp3_files = vec![];
		for entry in std::fs::read_dir(".").unwrap() {
			let entry = entry.unwrap();

			match entry.path().extension() {
				Some(ext) if ext == OsStr::new("mp3") => {
					mp3_files.push(entry.path());
				}
				_ => (),
			}
		}

		// Channel for communicating back to the GUI thread
		let (tx, rx) = sync_channel::<GenMsg>(8); //gen- idk why 8

		// Make me a sink, baby
		let sink = Sink::default();

		let cb_tx = tx.clone();
		let cb_ctx = cc.egui_ctx.clone();
		sink.on_callback(Some(move |cbi| sink_cb(cbi, &cb_tx, &cb_ctx)))
			.unwrap();

		let cb_err_tx = tx.clone();
		sink.on_err_callback(Some(move |err| sink_cb_err(err, &cb_err_tx)))
			.unwrap();

		Self {
			files: mp3_files,
			tx,
			rx,
			ctx: cc.egui_ctx.clone(),
			sink,
			state: GenState::Stopped,
			next: None,
			timekeeper: None,
			timekeeper_cancel: Arc::new(AtomicBool::new(false)),
		}
	}

	fn is_playing(&self) -> bool {
		//SAFTEY: if we lost the mutex it's so joever
		let sink_playing = self.sink.is_playing().unwrap();
		let state_playing = GenState::Playing == self.state;

		if sink_playing != state_playing {
			eprintln!("[WARN] sink state and genstate desynced");
		}

		state_playing
	}

	fn handle_events(&mut self) {
		loop {
			match self.rx.try_recv() {
				Ok(GenMsg::MediaEnded) => todo!(),
				Ok(GenMsg::FinishPause) => {
					self.sink.hard_pause().unwrap();
				}
				Ok(GenMsg::TimestampWakeup) => (),
				Err(TryRecvError::Disconnected) => panic!(),
				Err(TryRecvError::Empty) => break,
			}
		}
	}

	fn start_timekeeper(&mut self) {
		if self.timekeeper.is_none() {
			let cancel = self.timekeeper_cancel.clone();
			let tx = self.tx.clone();
			let ctx = self.ctx.clone();

			let hwnd = std::thread::spawn(move || loop {
				std::thread::sleep(Duration::from_millis(250));

				if cancel.load(Ordering::Relaxed) {
					break;
				}

				tx.send(GenMsg::TimestampWakeup).unwrap();
				ctx.request_repaint();
			});

			self.timekeeper = Some(hwnd);
		}
	}

	fn stop_timekeeper(&mut self) {
		self.timekeeper_cancel.store(true, Ordering::Release);
		// we drop it here and let it wakeup and die later
		let _ = self.timekeeper.take();
	}

	fn change_media<P: Into<PathBuf>>(&mut self, path: P) {
		if self.is_playing() {
			self.sink.pause().unwrap();
		}
		self.state = GenState::Stopped;

		self.next = Some(path.into());
	}

	fn pause(&mut self) {
		if self.state == GenState::Playing {
			self.state = GenState::Paused;
		}
		self.sink.pause().unwrap();
	}

	fn unpause(&mut self) {
		if self.state == GenState::Paused {
			self.state = GenState::Playing;
		}
		self.sink.resume().unwrap();
	}

	fn start_playing(&mut self) {
		if let Some(path) = self.next.as_deref() {
			let file = File::open(path).unwrap();
			let symph = Symph::try_new(file, &Default::default()).unwrap();

			self.state = GenState::Playing;
			self.sink.load(symph, true).unwrap();
			self.start_timekeeper();
		}
	}
}

impl eframe::App for GensMusic {
	fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
		self.handle_events();

		egui::TopBottomPanel::bottom("controls")
			.frame(egui::Frame::default().inner_margin(8.0))
			.show(ctx, |ui| {
				if self.is_playing() {
					if ui.button("Pause").clicked() {
						self.pause();
					}
				} else {
					if ui.button("Play").clicked() {
						match self.state {
							GenState::Paused => self.unpause(),
							GenState::Playing => panic!(),
							GenState::Stopped => self.start_playing(),
						}
					}
				}

				if let Some(path) = self.next.as_deref() {
					ui.horizontal(|ui| {
						if self.is_playing() {
							let timestamp = self.sink.get_timestamp().unwrap();
							ui.label(format!(
								"{}/{}",
								timestamp.current.as_secs(),
								timestamp.total.as_secs()
							));
						} else {
							ui.label("0 / 0");
						}

						ui.separator();

						ui.label(path.to_string_lossy());
					});
				}
			});

		egui::CentralPanel::default().show(ctx, |ui| {
			TableBuilder::new(ui)
				.striped(true)
				.column(Column::remainder())
				.header(20.0, |mut header| {
					header.col(|ui| {
						ui.heading("Path");
					});
				})
				.body(|mut body| {
					// So clearly a clone here is unoptimal, but.
					for file in self.files.clone() {
						body.row(20.0, |mut row| {
							row.col(|ui| {
								if ui.label(file.to_string_lossy()).clicked() {
									self.change_media(file);
								}
							});
						});
					}
				});
		});
	}
}

fn sink_cb(cbi: CallbackInfo, tx: &SyncSender<GenMsg>, ctx: &egui::Context) {
	match cbi {
		CallbackInfo::SourceEnded => tx.send(GenMsg::MediaEnded).unwrap(),
		CallbackInfo::PauseEnds(_) => tx.send(GenMsg::FinishPause).unwrap(),
		_ => todo!(),
	}

	ctx.request_repaint();
}

fn sink_cb_err(_err: raplay::Error, tx: &SyncSender<GenMsg>) {}
