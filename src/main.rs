use std::{
	collections::VecDeque,
	ffi::OsStr,
	fs::File,
	path::PathBuf,
	sync::mpsc::{sync_channel, Receiver, TryRecvError},
	time::Duration,
};

use eframe::{egui, NativeOptions};
use egui_extras::{Column, TableBuilder};
use raplay::{
	source::{Source, Symph},
	Timestamp,
};
use sounder::Sounder;
use timekeeper::Timekeeper;

mod sounder;
mod timekeeper;

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
	/// Sent by the [Timekeeper] to wake us up and update the elapsed time
	Timetick,
}

#[derive(Debug, PartialEq)]
pub enum GenState {
	/// We'll probably only ever be in this state on first boot.
	Stopped,
	/// There is no media playing
	Paused,
	/// There is media playing
	Playing,
}

pub struct Current {
	path: PathBuf,
	timestamp: Timestamp,
}

impl Current {
	pub fn new<P: Into<PathBuf>>(path: P) -> Self {
		Self {
			path: path.into(),
			timestamp: Timestamp {
				current: Duration::ZERO,
				total: Duration::ZERO,
			},
		}
	}

	pub fn new_with_duration<P: Into<PathBuf>>(path: P, duration: Duration) -> Self {
		Self {
			path: path.into(),
			timestamp: Timestamp {
				current: Duration::ZERO,
				total: duration,
			},
		}
	}
}

struct GensMusic {
	files: Vec<PathBuf>,

	/// Communication from other threads
	rx: Receiver<GenMsg>,

	/// Sound output and control via raplay
	sounder: Sounder,
	timekeeper: Timekeeper,
	state: GenState,

	queue: VecDeque<PathBuf>,
	current: Option<Current>,
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

		let sounder = Sounder::new(cc.egui_ctx.clone(), tx.clone());

		Self {
			files: mp3_files,
			rx,
			sounder,
			state: GenState::Stopped,
			timekeeper: Timekeeper::new(cc.egui_ctx.clone(), tx),
			current: None,
			queue: VecDeque::new(),
		}
	}

	fn is_playing(&self) -> bool {
		self.state == GenState::Playing
	}

	fn handle_events(&mut self) {
		loop {
			match self.rx.try_recv() {
				Ok(GenMsg::MediaEnded) => {
					self.advance_queue();
					self.unpause();
				}
				Ok(GenMsg::FinishPause) => {
					self.sounder.finish_pause();
				}
				Ok(GenMsg::Timetick) => {
					if let Some(stamp) = self.sounder.timestamp() {
						if let Some(cur) = self.current.as_mut() {
							cur.timestamp = stamp;
						}
					}
				}
				Err(TryRecvError::Disconnected) => panic!(),
				Err(TryRecvError::Empty) => break,
			}
		}
	}

	/// Call this when you are starting a new queue or the current media soruce has ended
	fn advance_queue(&mut self) {
		let next = self.queue.pop_front();

		match next {
			Some(path) => {
				let file = File::open(&path).unwrap();
				let symph = Symph::try_new(file, &Default::default()).unwrap();
				let timestamp = symph.get_time().unwrap();

				self.sounder.load(symph);
				self.current = Some(Current { path, timestamp });
			}
			None => {
				self.sounder.pause();
				self.current = None;
			}
		}
	}

	fn pause(&mut self) {
		if self.state == GenState::Playing {
			self.state = GenState::Paused;
		}
		self.sounder.pause();
		self.timekeeper.stop();
	}

	fn unpause(&mut self) {
		if self.state == GenState::Paused {
			self.state = GenState::Playing;
		}
		self.sounder.play();
		self.timekeeper.start(Duration::from_millis(100));
	}
}

impl eframe::App for GensMusic {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
							GenState::Stopped => self.unpause(),
						}
					}
				}

				if let Some(current) = self.current.as_ref() {
					ui.horizontal(|ui| {
						ui.label(format!(
							"{} / {}",
							current.timestamp.current.as_secs(),
							current.timestamp.total.as_secs()
						));

						ui.separator();

						ui.label(current.path.to_string_lossy());
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
					let mut should_advance = false;

					for (idx, file) in self.files.iter().enumerate() {
						body.row(20.0, |mut row| {
							row.col(|ui| {
								if ui.label(file.to_string_lossy()).clicked() {
									self.queue.clear();
									self.queue
										.extend(self.files[idx..].iter().map(<_>::to_owned));
									should_advance = true;
								}
							});
						});
					}

					if should_advance {
						self.advance_queue();
					}
				});
		});
	}
}
