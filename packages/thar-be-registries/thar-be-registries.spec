%global _cross_first_party 1
%undefine _debugsource_packages
%global cross_generate_sbom %{shrink: \
  mkdir -p %{_builddir}/sbom-temp && \
  sbomtool generate \
    --name thar-be-registries \
    --out-dir %{_builddir}/sbom-temp \
    --build-dir %{_builddir}/sources \
    --spdx --cyclonedx}

Name: %{_cross_os}thar-be-registries
Version: 0.0
Release: 0%{?dist}
Summary: Registry settings agent
License: Apache-2.0 OR MIT
URL: https://github.com/bottlerocket-os/bottlerocket
Source1: thar-be-registries-toml
BuildRequires: %{_cross_os}glibc-devel

%description
%{summary}.

%prep
%setup -T -c
%cargo_prep

%build
%cargo_build --manifest-path %{_builddir}/sources/Cargo.toml \
    -p thar-be-registries

%install
install -d %{buildroot}%{_cross_bindir}
install -p -m 0755 %{__cargo_outdir}/thar-be-registries %{buildroot}%{_cross_bindir}

install -d %{buildroot}%{_cross_templatedir}
install -p -m 0644 %{S:1} %{buildroot}%{_cross_templatedir}

%files
%{_cross_bindir}/thar-be-registries
%{_cross_templatedir}/thar-be-registries-toml

%changelog
