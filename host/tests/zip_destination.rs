use smol_epub::{
    scratch::HeapScratch,
    zip::{self, ZipEntry},
};

fn extract(method: u16, declared: u32) -> Result<Vec<u8>, &'static str> {
    let source = b"caller-owned output caller-owned output";
    let payload = if method == zip::METHOD_STORED {
        source.to_vec()
    } else {
        miniz_oxide::deflate::compress_to_vec(source, 6)
    };
    let mut file = vec![0u8; 30];
    file[..4].copy_from_slice(&0x04034b50u32.to_le_bytes());
    file.extend_from_slice(&payload);
    let entry = ZipEntry {
        method,
        comp_size: payload.len() as u32,
        uncomp_size: declared,
        ..ZipEntry::EMPTY
    };
    let mut output = vec![0xa5; declared as usize];
    let pointer = output.as_ptr();
    let result = zip::extract_entry_with_buffer_and_scratch(
        &entry,
        0,
        |off, dst| {
            let tail = file.get(off as usize..).ok_or("past end")?;
            let n = dst.len().min(tail.len()).min(7);
            dst[..n].copy_from_slice(&tail[..n]);
            Ok::<_, &'static str>(n)
        },
        |n| {
            assert_eq!(n, output.len());
            Ok(output.as_mut_slice())
        },
        HeapScratch::zeroed,
    )?;
    assert_eq!(result.as_ptr(), pointer);
    assert_eq!(result, source);
    Ok(output)
}

#[test]
fn stored_and_deflate_extract_directly_into_caller_storage_with_short_reads() {
    for method in [zip::METHOD_STORED, zip::METHOD_DEFLATE] {
        extract(method, 39).unwrap();
    }
}

#[test]
fn declared_size_mismatch_is_not_published() {
    for method in [zip::METHOD_STORED, zip::METHOD_DEFLATE] {
        assert!(extract(method, 45).is_err());
        assert!(extract(method, 3).is_err());
    }
}
