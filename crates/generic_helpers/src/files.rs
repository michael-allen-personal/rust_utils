use std::{
    fs::File,
    io::{self, BufReader},
    mem::size_of,
    path::Path,
};

pub trait MaxVecCapacity: Sized {
    /// Estimate the max size of a vector from the size of the file
    /// This function assumes that the data in the file is mostly of
    /// the type this trait is implemented for.
    fn estimate_max_vec_capacity_from_file<P: AsRef<Path>>(
        path: P,
    ) -> Result<usize, std::io::Error> {
        let type_bytes = size_of::<Self>();
        let file_bytes = File::open(path)?.metadata()?.len() as usize;
        Ok(file_bytes / type_bytes)
    }
}

pub fn buf_reader_from_path<P: AsRef<Path>>(path: P) -> io::Result<BufReader<File>> {
    let f = File::open(path)?;
    Ok(BufReader::new(f))
}
