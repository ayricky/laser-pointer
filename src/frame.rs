//! A successful latency wait belongs to its owner until that owner presents.

use windows::core::{Error, Result};
use windows::Win32::Foundation::{E_FAIL, HANDLE, WAIT_FAILED, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{WaitForSingleObject, INFINITE};

#[derive(Debug, PartialEq)]
pub enum Schedule {
    Idle,
    Wait,
    Render,
}

pub fn schedule(needs_frame: bool, frame_slot: bool) -> Schedule {
    match (needs_frame, frame_slot) {
        (false, _) => Schedule::Idle,
        (true, false) => Schedule::Wait,
        (true, true) => Schedule::Render,
    }
}

pub fn acquire(frame_slot: &mut bool, wait: impl FnOnce() -> Result<()>) -> Result<()> {
    if !*frame_slot {
        wait()?;
        *frame_slot = true;
    }
    Ok(())
}

pub fn present<R>(frame_slot: &mut bool, present: impl FnOnce() -> R) -> R {
    debug_assert!(*frame_slot);
    *frame_slot = false;
    present()
}

/// Direct paints must take the same semaphore slot as frames driven by the message loop.
pub unsafe fn wait(handle: HANDLE) -> Result<()> {
    match WaitForSingleObject(handle, INFINITE) {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_FAILED => Err(Error::from_thread()),
        _ => Err(E_FAIL.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skipped_frames_keep_the_slot_until_a_present() -> Result<()> {
        let mut frame_slot = false;
        assert_eq!(schedule(true, frame_slot), Schedule::Wait);
        acquire(&mut frame_slot, || Ok(()))?;

        // Input can make an owner idle after its wait, before it presents.
        assert_eq!(schedule(false, frame_slot), Schedule::Idle);
        assert_eq!(schedule(true, frame_slot), Schedule::Render);
        acquire(&mut frame_slot, || {
            panic!("a held slot must not be waited on twice")
        })?;

        let mut presents = 0;
        present(&mut frame_slot, || presents += 1);
        assert_eq!(presents, 1);
        assert_eq!(schedule(true, frame_slot), Schedule::Wait);

        // A paint outside the loop acquires and presents through the same bookkeeping.
        acquire(&mut frame_slot, || Ok(()))?;
        let result: Result<()> = present(&mut frame_slot, || Err(E_FAIL.into()));
        assert!(result.is_err());
        assert!(!frame_slot);
        Ok(())
    }

    #[test]
    fn failed_waits_and_recreated_chains_start_without_a_held_slot() {
        let mut frame_slot = false;
        assert!(acquire(&mut frame_slot, || Err(E_FAIL.into())).is_err());
        assert!(!frame_slot);

        acquire(&mut frame_slot, || Ok(())).unwrap();
        frame_slot = false;
        assert_eq!(schedule(true, frame_slot), Schedule::Wait);
    }
}
