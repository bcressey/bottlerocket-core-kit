%global cxxopts_ver 3.2.0
%global rpmalloc_ver 1.4.5
%global isal_commit fb41a9673f23cd965d1f474450ebf80a0d660c8f

Name: %{_cross_os}rapidgzip
Version: 0.14.3
Release: 1%{?dist}
Summary: Rapid implementation of gzip
License: ¯\_(ツ)_/¯
Source0: https://github.com/mxmlnkn/rapidgzip/archive/refs/tags/rapidgzip-v%{version}.tar.gz
Source1: https://github.com/jarro2783/cxxopts/archive/refs/tags/v%{cxxopts_ver}.tar.gz#/cxxopts-%{cxxopts_ver}.tar.gz
Source2: https://github.com/mjansson/rpmalloc/archive/refs/tags/%{rpmalloc_ver}.tar.gz#/rpmalloc-%{rpmalloc_ver}.tar.gz
Source3: https://github.com/mxmlnkn/isa-l/archive/%{isal_commit}.tar.gz#/isa-l-%{isal_commit}.tar.gz
BuildRequires: %{_cross_os}glibc-devel
BuildRequires: %{_cross_os}libz-devel
Requires: %{_cross_os}libstdc++
Requires: %{_cross_os}libz

Patch0001: 0001-build-skip-tests-and-benchmarks.patch
Patch0002: 0002-build-unset-hardening-cflags.patch

%description
%{summary}.

%prep
%autosetup -n rapidgzip-rapidgzip-v%{version} -p1
tar -xf %{S:1} --strip-components=1 -C src/external/cxxopts
tar -xf %{S:2} --strip-components=1 -C src/external/rpmalloc
tar -xf %{S:3} --strip-components=1 -C src/external/isa-l

%build
%{cross_cmake} . \
  -DUSE_SYSTEM_ZLIB:BOOL=ON \
%if "%{_cross_arch}" == "x86_64"
  -DWITH_ISAL:BOOL=ON \
%else
  -DWITH_ISAL:BOOL=OFF \
%endif
  -DCMAKE_INSTALL_PREFIX:PATH=%{_cross_prefix} \
  -G Ninja

cmake --build .

%install
install -d %{buildroot}%{_cross_bindir}
install -p -m 0755 src/tools/rapidgzip %{buildroot}%{_cross_bindir}

%files
%license LICENSE-MIT LICENSE-APACHE
%{_cross_attribution_file}
%{_cross_bindir}/rapidgzip
