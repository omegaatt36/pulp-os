// Write operations shared with host regression tests. Keep cleanup inside the
// operation: every opened file is closed once, even after a write or seek error.
macro_rules! finish_write {
    ($inner:expr, $file:expr, $result:expr, $source:expr) => {{
        let result = $result;
        if result.is_err() {
            $inner.discard_block_cache();
        }
        let closed = $inner
            .mgr
            .close_file($file)
            .await
            .map_err(|_| Error::new(ErrorKind::WriteFailed, $source));
        if closed.is_err() {
            $inner.discard_block_cache();
        }
        // The first failure is more informative than a later flush failure.
        result.and(closed)
    }};
}

macro_rules! op_write {
    ($inner:expr, $dir:expr, $name:expr, $data:expr) => {
        match $inner
            .mgr
            .open_file_in_dir($dir, $name, Mode::ReadWriteCreateOrTruncate)
            .await
        {
            Err(_) => Err(Error::new(ErrorKind::OpenFile, "write")),
            Ok(file) => {
                let result = if ($data).is_empty() {
                    Ok(())
                } else {
                    $inner
                        .mgr
                        .write(file, $data)
                        .await
                        .map_err(|_| Error::new(ErrorKind::WriteFailed, "write"))
                };
                finish_write!($inner, file, result, "write:close")
            }
        }
    };
}

macro_rules! op_append {
    ($inner:expr, $dir:expr, $name:expr, $data:expr) => {
        match $inner
            .mgr
            .open_file_in_dir($dir, $name, Mode::ReadWriteCreateOrAppend)
            .await
        {
            Err(_) => Err(Error::new(ErrorKind::OpenFile, "append")),
            Ok(file) => {
                let result = if ($data).is_empty() {
                    Ok(())
                } else {
                    $inner
                        .mgr
                        .write(file, $data)
                        .await
                        .map_err(|_| Error::new(ErrorKind::WriteFailed, "append"))
                };
                finish_write!($inner, file, result, "append:close")
            }
        }
    };
}

macro_rules! op_write_at {
    ($inner:expr, $dir:expr, $name:expr, $offset:expr, $data:expr) => {
        match $inner
            .mgr
            .open_file_in_dir($dir, $name, Mode::ReadWriteCreateOrAppend)
            .await
        {
            Err(_) => Err(Error::new(ErrorKind::OpenFile, "write_at")),
            Ok(file) => {
                let result = match $inner.mgr.file_seek_from_start(file, $offset) {
                    Ok(()) => $inner
                        .mgr
                        .write(file, $data)
                        .await
                        .map_err(|_| Error::new(ErrorKind::WriteFailed, "write_at")),
                    Err(_) => Err(Error::new(ErrorKind::SeekFailed, "write_at")),
                };
                finish_write!($inner, file, result, "write_at:close")
            }
        }
    };
}
