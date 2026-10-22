//! The Store build's licence. The Store sells it with a week's free trial but leaves enforcing it
//! to the app, so once the trial is over the laser won't turn on until it's bought. The plain exe
//! has no licence and is never limited, and nor is the Store build when its licence can't be
//! read: a paying user must never be locked out by an error.

use std::sync::Mutex;

use windows::core::{w, Interface, HSTRING, PCWSTR};
use windows::Services::Store::StoreContext;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows::Win32::UI::Shell::{IInitializeWithWindow, ShellExecuteW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::settings;

/// The product's page, where the trial is bought.
const PRODUCT_PAGE: &str = "ms-windows-store://pdp/?productid=9MT6DPQK4631";

/// 100 ns ticks, as Windows counts time.
const DAY: i64 = 24 * 60 * 60 * 10_000_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Licence {
    /// Bought, the plain exe, or a licence that couldn't be read.
    Full,
    /// In the free trial, with this many days left, counting a part day as one.
    Trial(u32),
    /// Not bought: the trial is over, or the Store has no active licence for this user.
    Unlicensed { trial_over: bool },
}

static LICENCE: Mutex<Licence> = Mutex::new(Licence::Full);

pub fn licence() -> Licence {
    *LICENCE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Reads the licence again on a worker thread, as the Store may take a moment to answer, then
/// calls `then` there with what it found. `owner` is the window the Store's dialogs would belong
/// to; a desktop app has to name one or the Store may answer wrongly.
pub fn check(owner: HWND, then: impl FnOnce(Licence) + Send + 'static) {
    if settings::package_family().is_none() {
        return then(Licence::Full);
    }
    let owner = owner.0 as isize;
    std::thread::spawn(move || {
        let licence = read(HWND(owner as _)).unwrap_or(Licence::Full);
        *LICENCE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = licence;
        then(licence);
    });
}

fn read(owner: HWND) -> windows::core::Result<Licence> {
    let context = StoreContext::GetDefault()?;
    unsafe { context.cast::<IInitializeWithWindow>()?.Initialize(owner)? };
    let licence = context.GetAppLicenseAsync()?.join()?;
    let trial = licence.IsTrial()?;
    let left = if trial { licence.ExpirationDate()?.UniversalTime - now() } else { 0 };
    Ok(classify(licence.IsActive()?, trial, left))
}

/// What the Store's answer means. `left` is how long the trial has to run, in 100 ns ticks.
fn classify(active: bool, trial: bool, left: i64) -> Licence {
    match (active, trial) {
        (false, trial_over) => Licence::Unlicensed { trial_over },
        (true, false) => Licence::Full,
        // The Store doesn't say what an expired trial looks like, so the date decides too.
        (true, true) if left <= 0 => Licence::Unlicensed { trial_over: true },
        (true, true) => Licence::Trial(((left + DAY - 1) / DAY) as u32),
    }
}

/// Now, in the 100 ns ticks since 1601 that a WinRT DateTime counts.
fn now() -> i64 {
    let time = unsafe { GetSystemTimeAsFileTime() };
    ((time.dwHighDateTime as i64) << 32) | time.dwLowDateTime as i64
}

/// The product's page in the Store, where the app is bought.
pub fn product_page() -> String {
    PRODUCT_PAGE.into()
}

pub fn open_product_page() {
    unsafe {
        let _ = ShellExecuteW(None, w!("open"), &HSTRING::from(PRODUCT_PAGE), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trial_counts_part_days_and_ends_on_its_date() {
        assert_eq!(classify(true, true, 7 * DAY), Licence::Trial(7));
        assert_eq!(classify(true, true, 6 * DAY + 1), Licence::Trial(7));
        assert_eq!(classify(true, true, 1), Licence::Trial(1));
        assert_eq!(classify(true, true, 0), Licence::Unlicensed { trial_over: true });
        assert_eq!(classify(true, true, -DAY), Licence::Unlicensed { trial_over: true });
        assert_eq!(classify(false, true, 3 * DAY), Licence::Unlicensed { trial_over: true });
    }

    #[test]
    fn bought_is_full_and_no_licence_is_not() {
        assert_eq!(classify(true, false, 0), Licence::Full);
        assert_eq!(classify(false, false, 0), Licence::Unlicensed { trial_over: false });
    }
}
