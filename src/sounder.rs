use std::sync::mpsc::SyncSender;

use eframe::egui::Context;
use raplay::{source::Symph, CallbackInfo, Sink, Timestamp};

use crate::GenMsg;

pub struct Sounder {
	sink: Sink,
}

impl Sounder {
	pub fn new(ctx: Context, tx: SyncSender<GenMsg>) -> Self {
		// Make me a sink, baby
		let sink = Sink::default();

		let cb_tx = tx.clone();
		let cb_ctx = ctx.clone();
		sink.on_callback(Some(move |cbi| sink_cb(cbi, &cb_tx, &cb_ctx)))
			.unwrap();

		let cb_err_tx = tx;
		sink.on_err_callback(Some(move |err| sink_cb_err(err, &cb_err_tx)))
			.unwrap();

		Self { sink }
	}

	/// Load an audio source preparing it for playback.
	pub fn load(&mut self, symph: Symph) {
		self.sink.load(symph, false).unwrap();
		let info = self.sink.get_info();

		println!(
			"{}hz // {} // {}ch",
			info.sample_rate, info.sample_format, info.channel_count
		);
	}

	pub fn timestamp(&self) -> Option<Timestamp> {
		self.sink.get_timestamp().ok()
	}

	/// Plays the current audio source.
	pub fn play(&self) {
		self.sink.resume().unwrap();
	}

	/// Stops playback of the current source and schedules the loop feeding the
	/// CPU to stop. You have to listen for [GenMsg::FinishPause] and call
	/// [Self::finish_pause] when you get it
	pub fn pause(&self) {
		self.sink.pause().unwrap();
	}

	/// Call when you receive [GenMsg::FinishPause]. We need to tell the audio
	/// thread to stop it's looping
	pub fn finish_pause(&self) {
		self.sink.hard_pause().unwrap();
	}

	/// Sets the audio output volume. Value should be in the range 0-100.
	/// Values outside of this range (higher than 100) will be capped to 100.
	pub fn set_volume(&mut self, vol: u8) {
		self.sink.volume(vol.min(100) as f32 / 100.0).unwrap();
	}
}

fn sink_cb(cbi: CallbackInfo, tx: &SyncSender<GenMsg>, ctx: &Context) {
	match cbi {
		CallbackInfo::SourceEnded => tx.send(GenMsg::MediaEnded).unwrap(),
		CallbackInfo::PauseEnds(_) => tx.send(GenMsg::FinishPause).unwrap(),
		_ => todo!(),
	}

	ctx.request_repaint();
}

fn sink_cb_err(_err: raplay::Error, _tx: &SyncSender<GenMsg>) {}
