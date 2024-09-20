use std::{
	collections::VecDeque,
	ffi::OsStr,
	fs::File,
	path::PathBuf,
	sync::mpsc::{sync_channel, Receiver, TryRecvError},
	time::Duration,
};

use audiotags::Tag;
use eframe::{
	egui::{self, ViewportBuilder},
	NativeOptions,
};
use egui_extras::{Column, TableBuilder};
use library::{Id, Library, Song};
use raplay::{
	source::{Source, Symph},
	Timestamp,
};
use sounder::Sounder;
use timekeeper::Timekeeper;

mod library;
mod sounder;
mod timekeeper;
mod ui;

fn main() {
	let nopt = NativeOptions {
		viewport: ViewportBuilder::default().with_inner_size([640.0, 480.0]),
		..Default::default()
	};

	eframe::run_native(
		"gensmusic",
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
	song: Song,
	timestamp: Timestamp,
}

impl Current {
	pub fn new(song: Song) -> Self {
		Self {
			song,
			timestamp: Timestamp {
				current: Duration::ZERO,
				total: Duration::ZERO,
			},
		}
	}

	pub fn new_with_duration(song: Song, duration: Duration) -> Self {
		Self {
			song,
			timestamp: Timestamp {
				current: Duration::ZERO,
				total: duration,
			},
		}
	}
}

struct GensMusic {
	library: Library,

	/// Communication from other threads
	rx: Receiver<GenMsg>,

	/// Sound output and control via raplay
	sounder: Sounder,
	timekeeper: Timekeeper,
	state: GenState,
	/// Volume is in the range 0 - 100
	volume: u8,

	queue: VecDeque<Song>,
	current: Option<Current>,
}

impl GensMusic {
	pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
		// Gather all MP3 files in the current diretory
		// Channel for communicating back to the GUI thread
		let (tx, rx) = sync_channel::<GenMsg>(8); //gen- idk why 8

		let sounder = Sounder::new(cc.egui_ctx.clone(), tx.clone());

		Self {
			library: Library::scan("/Users/gen/Lossy".into()),
			rx,

			sounder,
			state: GenState::Stopped,
			volume: 100,

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
			Some(song) => {
				let file = File::open(&song.path).unwrap();
				let symph = Symph::try_new(file, &Default::default()).unwrap();
				let timestamp = symph.get_time().unwrap();

				self.sounder.load(symph);
				self.current = Some(Current { song, timestamp });
			}
			None => {
				self.sounder.pause();
				self.current = None;
			}
		}
	}

	fn pause(&mut self) {
		self.state = GenState::Paused;

		self.sounder.pause();
		self.timekeeper.stop();
	}

	fn unpause(&mut self) {
		self.state = GenState::Playing;

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
				ui.horizontal(|ui| {
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

					ui.label("volume");
					let vol_slider = ui.add(egui::Slider::new(&mut self.volume, 0..=100));

					if vol_slider.changed() {
						self.sounder.set_volume(self.volume);
					}
				});

				if let Some(current) = self.current.as_ref() {
					ui.horizontal(|ui| {
						let cur = current.timestamp.current.as_secs();
						let tot = current.timestamp.total.as_secs();

						#[rustfmt::skip]
						ui.label(format!(
							"{}:{:02} / {}:{:02}",
							cur / 60, cur % 60,
							tot / 60, tot % 60
						));

						ui.separator();

						ui.label(&current.song.title);
						ui.separator();
						ui.label(&current.song.artist);
					});
				}
			});

		egui::CentralPanel::default().show(ctx, |ui| {
			TableBuilder::new(ui)
				.striped(true)
				.column(Column::auto())
				.column(Column::remainder())
				.column(Column::remainder())
				.column(Column::remainder())
				.header(20.0, |mut header| {
					header.col(|ui| {
						ui.heading("#");
					});
					header.col(|ui| {
						ui.heading("Title");
					});
					header.col(|ui| {
						ui.heading("Artist");
					});
					header.col(|ui| {
						ui.heading("Album");
					});
				})
				.body(|mut body| {
					let mut should_advance = false;

					for (idx, song) in self.library.songs().iter().enumerate() {
						body.row(20.0, |mut row| {
							row.col(|ui| {
								ui.label(&song.track_number.to_string());
							});

							row.col(|ui| {
								if ui.label(&song.title).clicked() {
									self.queue.clear();
									self.queue.extend(
										self.library.songs()[idx..].iter().map(<_>::to_owned),
									);
									should_advance = true;
								}
							});

							row.col(|ui| {
								ui.label(&song.artist);
							});

							row.col(|ui| {
								ui.label(&song.album_title);
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
