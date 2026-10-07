# The places CI's scripts share with the code (ADR-010, M5.27).
# Written by edel::places (crates/edel/src/places.rs); never edit it
# by hand. Sourced, not run.
# shellcheck disable=SC2034
settings_name=settings.toml
data_dir=/data/edel
run_dir=/run/edel
session_dir=/run/edel/session
state_file=/run/edel/session/state.toml
ready_file=/run/edel/session/ready
default_reached=/run/edel/default-reached
health_dir=/run/edel/health
confirmed_file=/run/edel/confirmed
started_file=/run/edel/started
greetd_live=/run/edel/greetd.toml
last_fallback=/data/edel/last-fallback.toml
share_dir=/usr/share/edel
esp_dir=/EFI/edel
home_session_log=.local/state/edel/session.log
system_log=/var/log/messages
