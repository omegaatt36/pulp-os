use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pulp_fontpack::{PackReader, ReadAt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceError {
    Injected,
    OutsideFile,
}

#[derive(Clone, Default)]
pub struct Control {
    pub reads: Rc<RefCell<Vec<(u64, usize)>>>,
    pub fail_offset: Rc<Cell<Option<u64>>>,
    pub disabled: Rc<Cell<bool>>,
}

pub struct ControlledSource {
    bytes: Vec<u8>,
    control: Control,
}

impl ReadAt for ControlledSource {
    type Error = SourceError;

    fn read_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), Self::Error> {
        self.control.reads.borrow_mut().push((offset, out.len()));
        if self.control.disabled.get() || self.control.fail_offset.get() == Some(offset) {
            return Err(SourceError::Injected);
        }
        let start = usize::try_from(offset).map_err(|_| SourceError::OutsideFile)?;
        let end = start
            .checked_add(out.len())
            .ok_or(SourceError::OutsideFile)?;
        let data = self.bytes.get(start..end).ok_or(SourceError::OutsideFile)?;
        out.copy_from_slice(data);
        Ok(())
    }
}

pub fn open(bytes: &[u8]) -> (PackReader<ControlledSource>, Control) {
    let control = Control::default();
    let source = ControlledSource {
        bytes: bytes.to_vec(),
        control: control.clone(),
    };
    let reader = PackReader::open(source, bytes.len() as u64).expect("handwritten pack opens");
    (reader, control)
}
