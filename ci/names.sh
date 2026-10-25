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
no_healthy_slot=/run/edel/no-healthy-slot
screen_file=/run/edel/screen
started_file=/run/edel/started
greetd_live=/run/edel/greetd.toml
last_fallback=/data/edel/last-fallback.toml
addons_dir=/data/addons
addons_dir_on_data=/addons
first_passwords=/data/edel/first-passwords
installed_mark=/data/edel/installed
greeter_dir=/data/edel/greeter
greeter_sessions=/data/edel/greeter/sessions.toml
share_dir=/usr/share/edel
keys_dir=/usr/share/edel/keys
firewall_rules=/usr/lib/edel/firewall/edel.nft
esp_dir=/EFI/edel
home_session_log=.local/state/edel/session.log
system_log=/var/log/messages
font_cache_dir=/usr/lib/fontconfig/cache
flathub_repo_file=/usr/share/edel/flathub.flatpakrepo
low_latency=/usr/share/edel/sound/low-latency.conf
home_low_latency_link=.config/pipewire/pipewire.conf.d/90-edel-low-latency.conf
