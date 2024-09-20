use std::{
	ffi::OsStr,
	path::{Path, PathBuf},
};

use audiotags::Tag;

pub struct Library {
	nextid: Id,

	directories: Vec<Directory>,
	songs: Vec<Song>,
}

impl Library {
	pub fn scan(path: PathBuf) -> Self {
		let mut this = Self {
			nextid: Id(0),
			directories: vec![],
			songs: vec![],
		};

		this.scan_dir(&path);

		this
	}

	pub fn songs(&self) -> &[Song] {
		&self.songs
	}

	fn scan_dir(&mut self, path: &Path) -> Id {
		let dir_id = self.nextid.adv();

		let mut dir = Directory {
			id: dir_id,
			path: path.to_owned(),
			directories: vec![],
			songs: vec![],
		};

		for entry in std::fs::read_dir(path).unwrap() {
			let entry = entry.unwrap();
			let meta = entry.metadata().unwrap();

			if meta.is_file() {
				if let Some(song_id) = self.read_song(&entry.path()) {
					dir.songs.push(song_id);
				}
			} else if meta.is_dir() {
				let child_dir_id = self.scan_dir(&entry.path());
				dir.directories.push(child_dir_id);
			}
		}

		self.directories.push(dir);

		dir_id
	}

	fn read_song(&mut self, path: &Path) -> Option<Id> {
		match path.extension() {
			Some(ext) if ext == OsStr::new("mp3") => {
				let Ok(tag) = Tag::new().read_from_path(path) else {
					println!("{} has no ID3 Tag", path.to_string_lossy());
					return None;
				};
				let title = tag.title().unwrap_or("untitled").to_owned();
				let artist = tag.artist().unwrap().to_owned();
				let album_title = tag.album_title().unwrap_or_default().to_owned();
				let disc_number = tag.disc_number().unwrap_or(1);
				let track_number = tag.track_number().unwrap_or(0);
				let id = self.nextid.adv();

				self.songs.push(Song {
					id,
					title,
					artist,
					album_title,

					disc_number,
					track_number,

					path: path.to_owned(),
				});

				Some(id)
			}
			_ => None,
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Id(pub usize);
impl Id {
	/// Advance the Id by one and return the previous value.
	pub fn adv(&mut self) -> Id {
		let old = self.clone();
		self.0 += 1;
		old
	}
}

struct Directory {
	id: Id,
	path: PathBuf,

	directories: Vec<Id>,
	songs: Vec<Id>,
}

#[derive(Clone, Debug)]
pub struct Song {
	pub id: Id,
	pub path: PathBuf,

	pub title: String,
	pub artist: String,
	pub album_title: String,

	pub disc_number: u16,
	pub track_number: u16,
}
