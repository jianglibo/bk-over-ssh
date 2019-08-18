use rustsync::*;
use rand::Rng;

use rand::distributions::Alphanumeric;

fn tt() {
  // Create 4 different random strings first.
  let chunk_size = 1000;
  let a = rand::thread_rng()
          .sample_iter(&Alphanumeric)
          .take(chunk_size)
          .collect::<String>();
  let b = rand::thread_rng()
          .sample_iter(&Alphanumeric)
          .take(50)
          .collect::<String>();
  let b_ = rand::thread_rng()
          .sample_iter(&Alphanumeric)
          .take(100)
          .collect::<String>();
  let c = rand::thread_rng()
          .sample_iter(&Alphanumeric)
          .take(chunk_size)
          .collect::<String>();

  // Now concatenate them in two different ways.

  let mut source = a.clone() + &b + &c;
  let mut modified = a + &b_ + &c;

  // Suppose we want to download `modified`, and we already have
  // `source`, which only differs by a few characters in the
  // middle.

  // We first have to choose a block size, which will be recorded
  // in the signature below. Blocks should normally be much bigger
  // than this in order to be efficient on large files.

  let block = [0; 32];

  // We then create a signature of `source`, to be uploaded to the
  // remote machine. Signatures are typically much smaller than
  // files, with just a few bytes per block.

  let source_sig = signature(source.as_bytes(), block).unwrap();

  // Then, we let the server compare our signature with their
  // version.

  let comp = compare(&source_sig, modified.as_bytes(), block).unwrap();

  // We finally download the result of that comparison, and
  // restore their file from that.

  let mut restored = Vec::new();
  restore_seek(&mut restored, std::io::Cursor::new(source.as_bytes()), vec![0; 1000], &comp).unwrap();
  assert_eq!(&restored[..], modified.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io};

    #[test]
    fn t_tt() {
        tt();
    }

    #[test]
    fn t_signature() -> Result<(), failure::Error> {
        let file_name = "/mnt/f/1804164.7z";
        let fr = fs::File::open(file_name)?;
        let reader = io::BufReader::new(fr);
        let block = [0; 32];
        let sig = signature(reader, block)?;
        println!("{:?}", sig);
        Ok(())
    }

}