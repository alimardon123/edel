# ADR-009: Enterprise means the same system at work

**Status:** Proposed
**Date:** 2026-10-06
**Deciders:** Alimardon
**Related:** ADR-003 (two years per release; one line amended here), ADR-005 (a longer-support channel; one line amended here), ADR-006 (settings file, fleets), ADR-007 (add-ons), ADR-008 (features, one way in), [ADR-010](ADR-010-parts-that-stand-alone.md) (parts that stand alone), [design principles](DESIGN-PRINCIPLES.md), [roadmap](ROADMAP.md)

## Context

On 2026-10-06 Alimardon asked two outside AI reviewers, a Claude chat and a GLM agent, to review the repository against their goal: to make Edel OS "consumer friendly and enterprise friendly", a "universal operating system for everyone", "currently much more focused on the consumer friendly aspect of it like UI" and on staying "a lightweight base on Alpine". They asked whether it could become "an enterprise grade production Linux system ... just like a Red Hat Enterprise Linux or Ubuntu", so that "developers can feel at home when they go into enterprise side of the things", for gamers, edge devices, developers and "usual daily people", with "smooth and premium user interaction ... whether it's UI, whether it's CLI". In the follow-up questions they added that this must not mean "heavy stuff" in the base, that the settings file exists so an administrator can "quickly launch" a copy of a machine, that they want "similar user experiences for everything", and that consumers "would need enterprise for great security" too.

A Claude session checked both reviews against the code on the same day. The first reviewer's findings held almost entirely: of the 16 checked, 15 were exact and one overstated. The second reviewer's direction was sound (enterprise pieces as optional features, the settings file as the fleet primitive, the edge as the way in, the same security for everyone), but many of its facts were wrong and several of its proposals broke rules this project already keeps: a systemd compatibility layer, a five-year channel built from "50 to 100" backported packages, a fleet agent that reports every five minutes, daily vulnerability scans on every machine and new per-domain commands. Both agreed on the main verdict: a general-purpose enterprise Linux in the Red Hat or Ubuntu sense (glibc, systemd, ten years of support, certifications, paid support) is not reachable on this base with one person and an AI, while an appliance-style system is.

Alimardon accepted the recommendations on 2026-10-06 ("Yes to all"), and added on support length: "I still like at least 10 years of the support for the enterprises. So we might consider this part a little bit more later", and on systemd: "System D is questionable, but it's not a bad idea for adapters", "if you think that's not necessary for now, we can postpone it", asking that the design stay "flexible so that later if we really need this kind of extra things, etc., we can add them later easily". ADR-010 records the modularity they asked for in the same message.

## Decision

### 1. The same system at home and at work, appliances first

Edel OS serves companies in two ways, both on the one base:

- **The same system at home and at work.** A developer's laptop, their VM, their container and the server they deploy to run the same base, the same `edel` tool and the same settings file. Workloads run in OCI containers, which also run unchanged on RHEL or Ubuntu hosts, and dev containers (M7.6) bring the distribution a project expects. Developers feel at home because every machine they touch is the same Edel OS, not because Edel OS imitates RHEL.
- **Appliances.** Container hosts, edge devices, kiosks and managed workstations that all run identical signed bits, each described by one settings file and copied with `edel settings export` and fleet images (M7.3).

A general-purpose enterprise distribution, where administrators install anything on the host and expect glibc and systemd, is not a goal now.

### 2. The same security for everyone

There is no enterprise-only security tier. Every image has signed updates that roll back on their own and a read-only system. From the fix-first round and M8 on, every image also has a clock that is right (M1.13), the shell's protocols closed to sandboxed apps (M5.22), encrypted data on laptop and desktop installs by default (M8.2), and a firewall that keeps inbound connections closed on desktops (M8.14). The deeper protections an enterprise asks for (a verified root, TPM unlock, audit, hardening checks) are built once in M11, and then reach every image that can use them. Settings says it in plain words ("Up to date. Data encrypted. Firewall on."), and `edel status` says the same to an administrator.

Honest limits, stated wherever we describe security: a read-only system protects the operating system's files, not a person's own files from a program they run; that is the app sandbox's job, and Flatpak apps get it.

### 3. The long channel: ten years as the goal, built when it can be carried

The base gives each yearly release two years (ADR-003). Alimardon wants at least ten years for enterprises. Ten years means keeping one release branch alive for eight more years with our own security backports for the kernel and every base package: this takes people or a sponsor, not new code. So the goal is recorded, and nothing may block it:

- channels already exist in `release.toml` and in the settings file (`system.channel`), so a long channel is one more channel;
- a release branch kept alive with CI, the monthly rebuild and the upgrade test is M8.5's mechanism;
- apps never link against the base (ADR-005), so a long-lived base does not freeze them.

The channel itself is M11.5, which starts only on Alimardon's word, when there is someone to carry it.

### 4. systemd stays out of the base; an adapter add-on stays possible

systemd stays out of the plan. People coming from RHEL or Ubuntu find their own tools inside containers and dev containers, and a short page in the docs (with M7.6) maps their habits: `systemctl restart` becomes `rc-service ... restart`, `journalctl` becomes the log files, and so on. A systemd adapter (`systemctl`-style commands and simple unit files mapped onto OpenRC and `edel`), as the second reviewer suggested, stays possible as an add-on (ADR-007), never in the base: M11.6, built when a real user needs it and Alimardon says so.

### 5. Doors kept open now, cheaply

Some choices are hard to change once people have installed Edel OS. They are taken now, before the first preview:

- **Slots stay byte-identical after install** (M1.12): slots are found by partition, get no new filesystem ID and are never resized, so a verified root (M11.1) can check them at every boot.
- **Release manifests expire and name their channel**, and are fetched only over `https://` or from a file (M3.8), so a stale or replayed list is refused, and a preview never reaches a stable machine.
- **Images name their own health checks** (M1.11), so a server's network or an edge device's job decides whether an update is good, not only whether services started.
- **A cloud VM reads its settings file from the cloud's user data** (M7.4). The user data is the plain settings file, not cloud-config, so cloud-init stays out of the plan.
- **Every image gets a bill of materials** (M8.12), made in CI from the package record CI already keeps.
- **Settings for fleets stay keys in the one settings file**, added within format 1 as ADR-008 allows. There is no second format and no management protocol.

### 6. No fleet agent, no telemetry

Fleets use what exists: fleet images (M7.3), the settings file, and `edel settings diff` run on a schedule in server images (M11.7), which writes what drifted to the machine's own log. No agent reports anywhere, and the hardware list grows only from reports people choose to send (M8.1).

## Options Considered

| Option | Verdict |
|---|---|
| A. A general-purpose enterprise Linux like RHEL or Ubuntu LTS: glibc, systemd, ten years, certifications, paid support | Not now: it needs a different base and an organisation. Its ten-year support length stays our goal (decision 3) |
| B. Enterprise appliances: container hosts, edge devices, kiosks, managed workstations | **Recommended**, with C: the design already fits, and the remaining security work is one milestone (M11) |
| C. The same system at home and at work | **Recommended**: one base, one tool, one file, workloads in containers |
| D. A systemd compatibility layer in the base | Rejected for the base: a moving target to chase for ever (Simple). Possible later as an add-on (M11.6) |
| E. A five-year channel by backporting "50 to 100 packages" | Rejected as costed: the kernel, Mesa, OpenSSL and firmware alone exceed it. The real cost is in decision 3 |
| F. A fleet agent, daily scans and compliance daemons on every machine | Rejected: background services and telemetry. CI checks images instead (M8.12, M11.4) |
| G. Certifications (FIPS 140-3, Common Criteria) | Not before there is a company and paying users |

Not built, and defended: an enterprise edition, an enterprise-only security tier, a management server, SELinux (Alpine ships none), new per-domain commands such as `edel service` (`[services]` is already a key, ADR-008), marketing numbers nobody measured.

## Consequences

- **Easier:** a developer's laptop and their servers are the same system; an appliance fleet is one file and one image; every security improvement reaches consumers too; the long channel and the systemd adapter can be added without changing a format.
- **Harder:** people who expect systemd on the host must use containers until M11.6, if ever; RHEL-certified vendor software that must run on the host is out of reach; ten years is a promise only once someone carries it.
- **Revisit:** the long channel and the systemd add-on when Alimardon says someone can carry them; certifications if a company forms; AppArmor in M11.3 once measured on Alpine.
- **Amends ADR-003:** "A longer-support channel for servers can come later, once there are users who need it" becomes a channel with ten years as the goal, built when it can be carried (M11.5).
- **Amends ADR-005:** "A longer-support base channel ... should wait until there are users asking for it" becomes the same.
- **Cost:** 25 new steps (M1.10 to M1.13, M3.6 to M3.9, M5.19 to M5.24, M8.12 to M8.14 and the eight of M11), most of them for later. None adds a process to a consumer image, apart from setting the clock at boot and the firewall rules.

## Action Items

1. [ ] The fix-first round, before M5.6: M3.6, M5.19 to M5.24, M1.10 to M1.13, M3.7 to M3.9.
2. [ ] One narrow review of the updater, the guard and the signing path before the first preview is published (M3.4; Alimardon, 2026-10-06).
3. [ ] A plain settings file from a cloud's user data (M7.4).
4. [ ] Encrypted data by default on laptop and desktop installs (M8.2).
5. [ ] A bill of materials and an advisory check for every image (M8.12); the firewall and a security status everyone can read (M8.14).
6. [ ] A "coming from Ubuntu or RHEL" page in the docs (M7.6).
7. [ ] Enterprise appliances and the long channel (M11).

## Principles check

- **Reliable:** health checks per image, manifests that expire, slots that can be verified and fallback without a watchdog keep unattended machines safe; promises stay goals until someone can keep them.
- **Simple:** one base, one tool, one file and one update path for home and work; enterprise pieces arrive as features and add-ons, never as a second system.
- **Efficient:** consumer images carry nothing new except setting the clock at boot and the firewall rules; checks run in CI, not on machines.
- **Scalable:** the same image and file from a laptop to a fleet; a long channel is one more channel.
- **Versatile:** developers bring their own distribution in containers; a systemd adapter can be added for those who need it.
- **Traded off:** Powerful and Versatile lose a host that runs anything built for RHEL; Simple pays 25 steps, most of them planned for later.
