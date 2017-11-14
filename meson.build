# Adatped from:
# https://gitlab.gnome.org/danigm/fractal/blob/6e2911f9d2353c99a18a6c19fab7f903c4bbb431/meson.build

project(
  'hammond', 'rust',
  version: '0.1.1',
  license: 'GPLv3',
)

hammond_version = meson.project_version()
version_array = hammond_version.split('.')
hammond_major_version = version_array[0].to_int()
hammond_minor_version = version_array[1].to_int()
hammond_version_micro = version_array[2].to_int()

hammond_prefix = get_option('prefix')
hammond_bindir = join_paths(hammond_prefix, get_option('bindir'))

install_data('hammond-gtk/resources/org.gnome.Hammond.desktop', install_dir : get_option('datadir') + '/applications')
install_data('hammond-gtk/resources/org.gnome.Hammond.appdata.xml', install_dir : get_option('datadir') + '/appdata')

cargo = find_program('cargo', required: false)
gresource = find_program('glib-compile-resources', required: false)
cargo_vendor = find_program('cargo-vendor', required: false)
cargo_script = find_program('scripts/cargo.sh')

cargo_release = custom_target('cargo-build',
                        build_by_default: true,
                        output: ['hammond'],
                        install: true,
                        install_dir: hammond_bindir,
                        command: [cargo_script, '@CURRENT_SOURCE_DIR@', '@OUTPUT@'])

run_target('release', command: ['scripts/release.sh',
                                meson.project_name() + '-' + hammond_version
                               ],
                                depends: [cargo_release])