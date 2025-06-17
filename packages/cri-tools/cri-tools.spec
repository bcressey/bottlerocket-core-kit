%global goproject github.com/kubernetes-sigs
%global gorepo cri-tools
%global goimport %{goproject}/%{gorepo}

%global gover 1.33.0
%global rpmver %{gover}

%global _dwz_low_mem_die_limit 0

Name: %{_cross_os}%{gorepo}
Version: %{rpmver}
Release: 1%{?dist}
Summary: CRI tools
License: Apache-2.0
URL: https://%{goimport}
Source0: https://%{goimport}/archive/v%{gover}/%{gorepo}-%{gover}.tar.gz
Source1: bundled-cri-tools-%{gover}.tar.gz
Source1000: clarify.toml

BuildRequires: git
BuildRequires: %{_cross_os}glibc-devel
Requires: %{name}(binaries)

%description
%{summary}.

%package bin
Summary: CRI tools binaries
Provides: %{name}(binaries)
Requires: (%{_cross_os}image-feature(no-fips) and %{name})
Conflicts: (%{_cross_os}image-feature(fips) or %{name}-fips-bin)

%description bin
%{summary}.

%package fips-bin
Summary: CRI tools binaries, FIPS edition
Provides: %{name}(binaries)
Requires: (%{_cross_os}image-feature(fips) and %{name})
Conflicts: (%{_cross_os}image-feature(no-fips) or %{name}-bin)

%description fips-bin
%{summary}.

%prep
%setup -n %{gorepo}-%{gover} -q
%setup -T -D -n %{gorepo}-%{version} -b 1

%build
%set_cross_go_flags
export GO_MAJOR="1.24"
go build -ldflags="${GOLDFLAGS}" -o crictl ./cmd/crictl
gofips build -ldflags="${GOLDFLAGS}" -o fips/crictl ./cmd/crictl

%install
install -d %{buildroot}%{_cross_bindir}
install -p -m 0755 crictl %{buildroot}%{_cross_bindir}

install -d %{buildroot}%{_cross_fips_bindir}
install -p -m 0755 crictl %{buildroot}%{_cross_fips_bindir}

%cross_scan_attribution --clarify %{S:1000} go-vendor vendor

%files
%license LICENSE
%{_cross_attribution_file}
%{_cross_attribution_vendor_dir}

%files bin
%{_cross_bindir}/crictl

%files fips-bin
%{_cross_fips_bindir}/crictl
