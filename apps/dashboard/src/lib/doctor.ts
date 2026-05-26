// Doctor surface (W10.T10.7/T10.8).
//
// Single import site for the dashboard's installation-health surface.
// Components MUST NOT call `invoke()` directly — they import from here.
//
// Wraps the `doctor_status` Tauri command that shells out to
// `mustard-rt run doctor --json` and parses the W10.T10.6 shape:
//
//   { overall: "ok"|"warn"|"fail", checks: [{ name, status, message, ... }] }
//
// Returns `Status = "fail"` when the binary is missing or the report cannot be
// parsed; the failure mode is encoded in the `error` field so the badge can
// render a meaningful tooltip rather than crash.

import { invoke } from "@tauri-apps/api/core";

export type DoctorOverall = "ok" | "warn" | "fail";
export type DoctorCheckStatus = "ok" | "warn" | "fail" | "skip";

export interface DoctorCheck {
  name: string;
  status: DoctorCheckStatus;
  message: string;
  details: string[];
}

export interface DoctorStatus {
  overall: DoctorOverall;
  checks: DoctorCheck[];
  /** Populated only when the subprocess could not produce a parseable report
   *  (binary missing, spawn failure, malformed JSON). */
  error: string | null;
}

interface RawDoctorCheck {
  name: string;
  status: string;
  message: string;
  details: string[];
}

interface RawDoctorStatus {
  overall: string;
  checks: RawDoctorCheck[];
  error: string | null;
}

function normaliseStatus(s: string): DoctorCheckStatus {
  return s === "ok" || s === "warn" || s === "fail" || s === "skip"
    ? s
    : "warn";
}

function normaliseOverall(s: string): DoctorOverall {
  return s === "ok" || s === "warn" || s === "fail" ? s : "warn";
}

/// Invoke the `doctor_status` Tauri command and normalise the response. The
/// status strings come back lowercased and the union types above gate downstream
/// callers (badge colour, tooltip layout). Empty `checks` paired with an `error`
/// is the explicit fail-soft signal — render red, show the error message.
export async function doctorStatus(projectPath: string): Promise<DoctorStatus> {
  const raw = await invoke<RawDoctorStatus>("doctor_status", {
    projectPath,
  });
  return {
    overall: normaliseOverall(raw.overall),
    checks: raw.checks.map((c) => ({
      name: c.name,
      status: normaliseStatus(c.status),
      message: c.message,
      details: c.details ?? [],
    })),
    error: raw.error,
  };
}
