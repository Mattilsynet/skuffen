use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// Én kilde til tjenestens tilstand (SKU-0021 R5).
///
/// Bare 1/0-flagg, ingen statusnivåer: readiness er sann når migrasjonene er
/// ferdige, NATS er tilkoblet og hver superviserte task er oppe. Samme flagg
/// kan leses av en metrikk senere uten omskriving.
#[derive(Clone, Default)]
pub struct Helse {
    tasks: Arc<RwLock<HashMap<String, Arc<AtomicBool>>>>,
    nats: Arc<AtomicBool>,
    migrert: Arc<AtomicBool>,
}

impl Helse {
    pub fn new() -> Self {
        Self::default()
    }

    /// Gir tasken sitt eget flagg. Supervisoren setter det mens `run_once`
    /// kjører og nullstiller det når den faller ut.
    pub fn registrer_task(&self, navn: &str) -> Arc<AtomicBool> {
        let mut tasks = self.tasks.write().expect("helse-låsen er aldri forgiftet");
        tasks
            .entry(navn.to_string())
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .clone()
    }

    pub fn sett_nats(&self, tilkoblet: bool) {
        self.nats.store(tilkoblet, Ordering::Relaxed);
    }

    pub fn sett_migrert(&self, ferdig: bool) {
        self.migrert.store(ferdig, Ordering::Relaxed);
    }

    pub fn er_klar(&self) -> bool {
        self.migrert.load(Ordering::Relaxed)
            && self.nats.load(Ordering::Relaxed)
            && self
                .tasks
                .read()
                .expect("helse-låsen er aldri forgiftet")
                .values()
                .all(|oppe| oppe.load(Ordering::Relaxed))
    }

    /// Navnene på tasks som er nede, til loggen bak et 503-svar.
    pub fn nede(&self) -> Vec<String> {
        self.tasks
            .read()
            .expect("helse-låsen er aldri forgiftet")
            .iter()
            .filter(|(_, oppe)| !oppe.load(Ordering::Relaxed))
            .map(|(navn, _)| navn.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_krever_migrasjoner_nats_og_alle_tasks() {
        let helse = Helse::new();
        let task = helse.registrer_task("command_listener");

        assert!(!helse.er_klar(), "ingenting er oppe ennå");

        helse.sett_migrert(true);
        helse.sett_nats(true);
        assert!(!helse.er_klar(), "tasken er fortsatt nede");

        task.store(true, Ordering::Relaxed);
        assert!(helse.er_klar());

        helse.sett_nats(false);
        assert!(!helse.er_klar(), "en tapt NATS-forbindelse er ikke klar");
    }

    #[test]
    fn samme_task_registreres_bare_en_gang() {
        let helse = Helse::new();
        let forste = helse.registrer_task("worker");
        forste.store(true, Ordering::Relaxed);

        // En supervisor-restart skal ikke lage et nytt, alltid-nede flagg.
        let andre = helse.registrer_task("worker");
        assert!(andre.load(Ordering::Relaxed));
        assert!(Arc::ptr_eq(&forste, &andre));
    }

    /// Registreringen ved oppstart og supervisorens `with_helse` må treffe
    /// samme navn. Gjør de ikke det, faller tasken stille ut av readiness.
    #[test]
    fn alle_tasknavn_er_unike() {
        let alle = crate::nats::supervisor::tasknavn::ALLE;
        let unike: std::collections::HashSet<&str> = alle.iter().copied().collect();
        assert_eq!(unike.len(), alle.len(), "to tasks deler navn: {alle:?}");
    }

    #[test]
    fn nede_navngir_taskene_som_mangler() {
        let helse = Helse::new();
        helse.registrer_task("query_listener");
        let oppe = helse.registrer_task("media_listener");
        oppe.store(true, Ordering::Relaxed);

        assert_eq!(helse.nede(), vec!["query_listener".to_string()]);
    }
}
