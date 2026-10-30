//! Windows only: ties child processes to the lifetime of Rhesis.
//!
//! Every spawned child is assigned to one job object configured with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. The job handle is never closed by
//! us, so Windows closes it when Rhesis exits for any reason (including a
//! crash) and terminates the children with it.

use std::{ffi::c_void, mem, os::windows::io::AsRawHandle, process::Child, ptr, sync::OnceLock};

use windows_sys::Win32::{
    Foundation::HANDLE,
    System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    },
};

struct JobHandle(HANDLE);

// SAFETY: a job handle is an opaque kernel handle value; the kernel allows
// using it from any thread.
unsafe impl Send for JobHandle {}
unsafe impl Sync for JobHandle {}

static JOB: OnceLock<Result<JobHandle, String>> = OnceLock::new();

fn create_job() -> Result<JobHandle, String> {
    // SAFETY: null attributes and name are valid arguments (anonymous job
    // with default security).
    let job = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
    if job.is_null() {
        return Err(format!(
            "CreateJobObjectW failed: {}",
            std::io::Error::last_os_error()
        ));
    }

    // SAFETY: the struct is plain data and all-zero is a valid value for it.
    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

    // SAFETY: `job` is a valid job handle, and the pointer/size describe
    // `info` for the extended limit information class.
    let ok = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const c_void,
            mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if ok == 0 {
        return Err(format!(
            "SetInformationJobObject failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(JobHandle(job))
}

/// Make `child` die together with this process.
pub fn kill_on_parent_exit(child: &Child) -> Result<(), String> {
    let job = JOB.get_or_init(create_job).as_ref().map_err(Clone::clone)?;

    // SAFETY: both handles are valid; the child handle stays owned by `child`.
    let ok = unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle() as HANDLE) };
    if ok == 0 {
        return Err(format!(
            "AssignProcessToJobObject failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}
