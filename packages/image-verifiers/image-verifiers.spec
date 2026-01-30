%global _cross_first_party 1
%undefine _debugsource_packages

Name: %{_cross_os}image-verifiers
Version: 0.1.0
Release: 1%{?dist}
Summary: Container image verification plugins for containerd
License: Apache-2.0 OR MIT
URL: https://github.com/bottlerocket-os/bottlerocket
BuildRequires: %{_cross_os}glibc-devel

Source1: thar-be-image-verifiers-toml
Source2: containerd-image-verifiers-toml

%description
%{summary}.

%package -n %{_cross_os}notation-image-verifier
Summary: Notation-based container image verification plugin
Requires: %{name}
Requires: %{_cross_os}notation
Requires: %{_cross_os}aws-signer-notation-plugin

%description -n %{_cross_os}notation-image-verifier
%{summary}.

%package -n %{_cross_os}digestion-image-verifier
Summary: Digest-based container image verification plugin
Requires: %{name}

%description -n %{_cross_os}digestion-image-verifier
%{summary}.

%prep
%setup -T -c
%cargo_prep

%build
%cargo_build --manifest-path %{_builddir}/sources/Cargo.toml \
    -p image-verifiers

%install
install -d %{buildroot}%{_cross_libexecdir}/civ/bin
install -p -m 0755 %{__cargo_outdir}/notation-image-verifier %{buildroot}%{_cross_libexecdir}/civ/bin
install -p -m 0755 %{__cargo_outdir}/digestion-image-verifier %{buildroot}%{_cross_libexecdir}/civ/bin

install -d %{buildroot}%{_cross_bindir}
install -p -m 0755 %{__cargo_outdir}/thar-be-image-verifiers %{buildroot}%{_cross_bindir}/thar-be-image-verifiers

install -d %{buildroot}%{_cross_templatedir}
install -p -m 0644 %{S:1} %{buildroot}%{_cross_templatedir}/thar-be-image-verifiers-toml
install -p -m 0644 %{S:2} %{buildroot}%{_cross_templatedir}/containerd-image-verifiers-toml

%files
%{_cross_bindir}/thar-be-image-verifiers
%{_cross_templatedir}/thar-be-image-verifiers-toml
%{_cross_templatedir}/containerd-image-verifiers-toml

%files -n %{_cross_os}notation-image-verifier
%{_cross_libexecdir}/civ/bin/notation-image-verifier

%files -n %{_cross_os}digestion-image-verifier
%{_cross_libexecdir}/civ/bin/digestion-image-verifier

%changelog
