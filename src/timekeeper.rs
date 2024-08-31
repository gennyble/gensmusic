use std::{
	sync::mpsc::{sync_channel, RecvTimeoutError, SyncSender},
	thread::JoinHandle,
	time::Duration,
};

use eframe::egui::Context;

use crate::GenMsg;

/// This struct keeps a thread and sends messages/requests repaints every
/// so often so the GUI thread wakes and updates the time elapsed.
pub struct Timekeeper {
	ctx: Context,
	gui_tx: SyncSender<GenMsg>,
	thread: Option<KeeperThread>,
}

struct KeeperThread {
	hwnd: JoinHandle<()>,
	stop_tx: SyncSender<()>,
}

impl Timekeeper {
	pub fn new(ctx: Context, gui_tx: SyncSender<GenMsg>) -> Self {
		Self {
			ctx,
			gui_tx,
			thread: None,
		}
	}

	/// Starts a new timekeeping thread or, if there is already one running,
	/// does nothing.
	pub fn start(&mut self, tick: Duration) {
		if self.thread.is_some() {
			return;
		}

		let (stop_tx, stop_rx) = sync_channel::<()>(1);
		let tx = self.gui_tx.clone();
		let ctx = self.ctx.clone();

		let hwnd = std::thread::spawn(move || loop {
			match stop_rx.recv_timeout(tick) {
				// Timeout is what happens in normal operation
				Err(RecvTimeoutError::Timeout) => (),
				// No send: we're probably shutting down
				Err(RecvTimeoutError::Disconnected) => break,
				// We've been told to shutdown
				Ok(_) => break,
			}

			tx.send(GenMsg::Timetick).unwrap();
			ctx.request_repaint();
		});

		self.thread = Some(KeeperThread { hwnd, stop_tx });
	}

	/// Stops and joins the timekeeping thread.
	pub fn stop(&mut self) {
		if let Some(thread) = self.thread.take() {
			// gen- If the receiver is disconnected it is probably that
			// the thread has terminated.
			thread.stop_tx.send(()).ok();
			thread.hwnd.join().unwrap();
		}
	}
}
