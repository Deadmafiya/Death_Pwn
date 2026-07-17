use std::collections::BTreeMap;

pub mod artifacts;

/// A scan/attack target — either a host or a URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub value: String,
}

/// A security finding surfaced during a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub severity: String,
    pub title: String,
    pub detail: String,
}

/// Accumulated knowledge about the current session. Mutated after each
/// execution and read by pipeline stages so follow-ups ("scan those ports")
/// resolve without re-stating the target.
#[derive(Debug, Clone, Default)]
pub struct SessionState {
    targets: Vec<Target>,
    hosts: Vec<String>,
    ports_by_host: BTreeMap<String, Vec<u16>>,
    services: Vec<String>,
    findings: Vec<Finding>,
    command_log: Vec<String>,
}

impl SessionState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_target(&mut self, target: Target) {
        if !self.targets.iter().any(|t| t.value == target.value) {
            self.targets.push(target);
        }
    }

    pub fn record_command(&mut self, command: &str) {
        self.command_log.push(command.to_string());
    }

    pub fn add_finding(&mut self, finding: Finding) {
        self.findings.push(finding);
    }

    pub fn add_service(&mut self, service: &str) {
        if !self.services.iter().any(|s| s == service) {
            self.services.push(service.to_string());
        }
    }

    pub fn add_ports(&mut self, host: &str, ports: Vec<u16>) {
        if !self.hosts.iter().any(|h| h == host) {
            self.hosts.push(host.to_string());
        }
        let entry = self.ports_by_host.entry(host.to_string()).or_default();
        for port in ports {
            if !entry.contains(&port) {
                entry.push(port);
            }
        }
        entry.sort_unstable();
    }

    pub fn targets(&self) -> &[Target] {
        &self.targets
    }

    pub fn hosts(&self) -> &[String] {
        &self.hosts
    }

    pub fn ports_by_host(&self) -> &BTreeMap<String, Vec<u16>> {
        &self.ports_by_host
    }

    pub fn services(&self) -> &[String] {
        &self.services
    }

    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    pub fn command_log(&self) -> &[String] {
        &self.command_log
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_targets_commands_and_dedups() {
        let mut s = SessionState::new();
        assert!(s.targets().is_empty());
        assert!(s.command_log().is_empty());

        s.add_target(Target { value: "192.168.1.1".to_string() });
        s.add_target(Target { value: "192.168.1.1".to_string() }); // duplicate ignored
        s.add_target(Target { value: "http://example.com".to_string() });

        s.record_command("nmap -sV 192.168.1.1");
        s.record_command("gobuster dir -u http://example.com");

        assert_eq!(s.targets().len(), 2);
        assert_eq!(s.targets()[0].value, "192.168.1.1");
        assert_eq!(
            s.command_log(),
            &[
                "nmap -sV 192.168.1.1".to_string(),
                "gobuster dir -u http://example.com".to_string(),
            ]
        );
    }
}
