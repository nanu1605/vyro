//! Other lighting programs (2.6.2; the owner, 2026-11-28: "lights are not working", with MSI
//! Center's Mystic Light on the PC, and choosing "Name it, I close it"). A vendor's lighting
//! program keeps its devices and writes over every colour OpenRGB sends, so the lights stay as
//! they were though Clokrix and OpenRGB both say they changed. Clokrix cannot win that fight and
//! never joins it: while lighting is on, the app reads the names of the services and processes
//! running, and the Lighting view names each program it recognises here, with how to stop it.
//! Nothing of that program is ever stopped, changed or opened by Clokrix.
//!
//! A program is listed only with a source for its names; the view's note still names the others
//! it knows of (iCUE, Synapse, G HUB, Armoury Crate).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A lighting program Clokrix recognises.
pub struct Program {
    pub name: &'static str,
    /// Pieces of its services' names or of the names Windows' Services shows, in lower case.
    pub services: &'static [&'static str],
    /// Its processes' image names, in lower case.
    pub images: &'static [&'static str],
    /// The vendor's own way to turn its lighting off, for the view's advice.
    pub remedy: &'static str,
}

/// The programs recognised.
pub const KNOWN: &[Program] = &[Program {
    // MSI Center's Mystic Light, and Dragon Center's and Mystic Light's own before it. Services:
    // Lian Li's L-Connect 3 x OpenRGB beta notes (set LightKeeperService, MSI Center Service,
    // MSI_Case_Service and Mystic_Light_Service to Manual) and py_rgb's README (stop
    // Mystic_Light_Service, MSI_Case_Service and LightKeeperService: they hold the controller).
    // Processes: SignalRGB's forum (Mystic Light starts LEDKeeper2) and MSI's own file names.
    name: "MSI Center's Mystic Light",
    services: &[
        "mystic_light",
        "mystic light",
        "lightkeeper",
        "ledkeeper",
        "msi_case_service",
        "msi center service",
    ],
    images: &["ledkeeper2.exe", "ledkeeper.exe", "lightkeeperservice.exe"],
    remedy: "remove Mystic Light in MSI Center's Features",
}];

/// A lighting program running now, as the Lighting view names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OtherLighting {
    pub name: String,
    /// Its services running, as Windows' Services names them.
    pub services: Vec<String>,
    /// Its processes running, when none of its services is.
    pub processes: Vec<String>,
    pub remedy: String,
}

/// The programs of [`KNOWN`] running now, from the services running (each its name and the name
/// Windows' Services shows) and the processes' image names; case is ignored.
pub fn found(services: &[(String, String)], images: &[String]) -> Vec<OtherLighting> {
    KNOWN
        .iter()
        .filter_map(|p| {
            let mut named: Vec<String> = Vec::new();
            for (key, display) in services {
                let (k, d) = (key.to_lowercase(), display.to_lowercase());
                if p.services.iter().any(|s| k.contains(s) || d.contains(s)) {
                    let shown = if display.trim().is_empty() {
                        key
                    } else {
                        display
                    };
                    if !named.contains(shown) {
                        named.push(shown.clone());
                    }
                }
            }
            let mut processes: Vec<String> = Vec::new();
            for image in images {
                let known = p.images.iter().any(|i| image.eq_ignore_ascii_case(i));
                if known && !processes.iter().any(|n| n.eq_ignore_ascii_case(image)) {
                    processes.push(image.clone());
                }
            }
            if named.is_empty() && processes.is_empty() {
                return None;
            }
            named.sort_by_key(|n| n.to_lowercase());
            processes.sort_by_key(|n| n.to_lowercase());
            Some(OtherLighting {
                name: p.name.to_owned(),
                // Its services say how to stop it; its processes only when no service runs.
                processes: if named.is_empty() {
                    processes
                } else {
                    Vec::new()
                },
                services: named,
                remedy: p.remedy.to_owned(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn services(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(k, d)| ((*k).to_owned(), (*d).to_owned()))
            .collect()
    }

    fn images(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn names_msi_s_lighting_services_whatever_their_generation() {
        // As the sources name them; display names constructed.
        let s = services(&[
            ("Mystic_Light_Service", "Mystic_Light_Service"),
            ("LightKeeperService", "LightKeeper Service"),
            ("MSI_Case_Service", "MSI_Case_Service"),
            ("MSI_Center_Service", "MSI Center Service"),
            ("Dnscache", "DNS Client"),
        ]);
        let f = found(&s, &images(&["explorer.exe", "LEDKeeper2.exe"]));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].name, "MSI Center's Mystic Light");
        assert_eq!(
            f[0].services,
            [
                "LightKeeper Service",
                "MSI Center Service",
                "MSI_Case_Service",
                "Mystic_Light_Service"
            ]
        );
        // Its services say how to stop it; the process is then not named.
        assert!(f[0].processes.is_empty());
        assert!(f[0].remedy.contains("Mystic Light"));

        // In another case, or by its display name only (key constructed).
        let f = found(&services(&[("mlsvc7", "MYSTIC LIGHT SERVICE")]), &[]);
        assert_eq!(f[0].services, ["MYSTIC LIGHT SERVICE"]);
        // A display name Windows does not have: the service's own name.
        let f = found(&services(&[("LEDKeeper2", " ")]), &[]);
        assert_eq!(f[0].services, ["LEDKeeper2"]);
    }

    #[test]
    fn names_its_process_when_no_service_of_it_runs() {
        let f = found(
            &[],
            &images(&["ledkeeper2.exe", "LEDKeeper2.exe", "LightKeeperService.exe"]),
        );
        assert_eq!(f.len(), 1);
        assert!(f[0].services.is_empty());
        // Each once, whatever its case.
        assert_eq!(f[0].processes, ["ledkeeper2.exe", "LightKeeperService.exe"]);
    }

    #[test]
    fn leaves_msi_s_other_programs_and_look_alikes_alone() {
        let s = services(&[
            ("RTCore64", "RTCore64"),
            ("NahimicService", "Nahimic service"),
            ("MSI Foundation Service", "MSI Foundation Service"),
            ("Lighthouse", "Lighthouse Keeper"),
        ]);
        let i = images(&[
            "MSIAfterburner.exe",
            "RTSS.exe",
            "MSI.CentralServer.exe",
            "OpenRGB.exe",
            "ledkeeper2.exe.bak",
        ]);
        assert_eq!(found(&s, &i), []);
        assert_eq!(found(&[], &[]), []);
    }
}
