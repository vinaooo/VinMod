#!/bin/bash
# Description: Script to compile a custom Linux kernel and package it into a .deb file for CachyOS
# Maintainer: Laio O. Seman <laio@iee.org>

# Initialize variables to store user choices
_cpusched_selection="cachyos"
_llvm_lto_selection="thin"
_tick_rate="1000"
_hugepage="madvise"
_o3_optimization="yes"
_os_optimization="no"
_performance_governor="yes"
_nr_cpus=""
_detected_threads=""
_bbr3="yes"
_zfs="no"
_march="native"
_preempt="preempt"
_tick_type="nohz_idle"

set_default_nr_cpus() {
    local threads

    threads=$(nproc --all 2>/dev/null)

    if ! [[ "$threads" =~ ^[0-9]+$ ]] || [ "$threads" -lt 1 ]; then
        threads=$(getconf _NPROCESSORS_ONLN 2>/dev/null)
    fi

    if ! [[ "$threads" =~ ^[0-9]+$ ]] || [ "$threads" -lt 1 ]; then
        threads=1
    fi

    _detected_threads="$threads"
    _nr_cpus=$((threads * 2))
}

check_deps() {

    # List of dependencies to check
    dependencies=(git libncurses-dev curl gawk flex bison openssl libssl-dev dkms libelf-dev libudev-dev libpci-dev libiberty-dev autoconf llvm bc rsync)

    # Iterate over dependencies and check each one
    for dep in "${dependencies[@]}"; do
        if dpkg -s "$dep" 2>/dev/null 1>&2; then
            #echo "Package $dep is installed."
            continue
        else
            #echo "Package $dep is NOT installed."
            sudo apt install -y "$dep"
        fi
    done

}

# Check if GCC is installed
check_gcc() {
    if ! [ -x "$(command -v gcc)" ]; then
        # Display error message if GCC is not installed
        echo "Error: GCC is not installed. Please install GCC and try again." >&2
        exit 1
    fi
}

die() {
    echo "Error: $*" >&2
    exit 1
}

download_file() {
    local url="$1"
    local output="$2"

    wget -c "$url" -O "$output" || die "Failed downloading: $url"
    [ -s "$output" ] || die "Downloaded file is empty: $output"
}

try_download_and_apply_patch() {
    local url="$1"
    local patch_file
    local source_label="$2"
    local patch_output

    patch_file="$(basename "$url")"
    if [ -n "$source_label" ]; then
        echo "Tentando patch do scheduler em $source_label: $patch_file"
    else
        echo "Tentando aplicar patch: $patch_file"
    fi

    if ! wget -c "$url" -O "$patch_file"; then
        rm -f "$patch_file"
        return 1
    fi

    [ -s "$patch_file" ] || {
        rm -f "$patch_file"
        return 1
    }

    # Try dry-run first to check if patch can be applied
    patch_output=$(patch -p1 --forward --batch --dry-run < "$patch_file" 2>&1)
    patch_exit_code=$?
    
    # Check if all hunks were ignored (patch already applied)
    if echo "$patch_output" | grep -q "Reversed (or previously applied) patch detected"; then
        if [ -n "$source_label" ]; then
            echo "Patch já estava aplicado (ou invertido) em $source_label: $patch_file"
        fi
        rm -f "$patch_file"
        return 0
    fi
    
    # Check if dry-run succeeded (exit code 0)
    if [ $patch_exit_code -eq 0 ]; then
        # Apply the patch for real
        if patch -p1 --forward --batch < "$patch_file" >/dev/null 2>&1; then
            if [ -n "$source_label" ]; then
                echo "Patch aplicado com sucesso a partir de $source_label: $patch_file"
            fi
            rm -f "$patch_file"
            return 0
        fi
    fi

    rm -f "$patch_file"
    return 1
}

download_and_apply_patch_candidates() {
    local url

    for url in "$@"; do
        if [[ "$url" == *"/sched-dev/"* ]]; then
            if try_download_and_apply_patch "$url" "sched-dev"; then
                return 0
            fi
        elif [[ "$url" == *"/sched/"* ]]; then
            if try_download_and_apply_patch "$url" "sched"; then
                return 0
            fi
        elif try_download_and_apply_patch "$url"; then
            return 0
        fi
    done

    return 1
}

download_and_apply_patch() {
    local url="$1"
    local patch_file

    patch_file="$(basename "$url")"
    if ! try_download_and_apply_patch "$url"; then
        if [[ "$patch_file" == *"bore"* || "$patch_file" == *"prjc"* || "$patch_file" == *"rt-i915"* ]]; then
            die "Failed applying scheduler patch: $patch_file (kernel $_kv_name). The patchset for this series may be behind this patchlevel. Try CPU Scheduler 'eevdf' or 'none', or select another kernel version."
        fi
        die "Failed applying patch: $patch_file"
    fi
}

# Original function used in the CachyOS mainline
init_script() {
    # Call the function before running the rest of the script
    check_gcc

    # Get CPU type from GCC and convert to uppercase
    MARCH=$(gcc -Q -march=native --help=target | grep -m1 march= | awk '{print toupper($2)}')

    # Check for specific CPU types and set MARCH variable accordingly
    case $MARCH in
    ZNVER1) MARCH="ZEN" ;;
    ZNVER2) MARCH="ZEN2" ;;
    ZNVER3) MARCH="ZEN3" ;;
    ZNVER4) MARCH="ZEN4" ;;
    BDVER1) MARCH="BULLDOZER" ;;
    BDVER2) MARCH="PILEDRIVER" ;;
    BDVER3) MARCH="STEAMROLLER" ;;
    BDVER4) MARCH="EXCAVATOR" ;;
    BTVER1) MARCH="BOBCAT" ;;
    BTVER2) MARCH="JAGUAR" ;;
    AMDFAM10) MARCH="MK10" ;;
    K8-SSE3) MARCH="K8SSE3" ;;
    BONNELL) MARCH="ATOM" ;;
    GOLDMONT-PLUS) MARCH="GOLDMONTPLUS" ;;
    SKYLAKE-AVX512) MARCH="SKYLAKEX" ;;
    MIVYBRIDGE)
        scripts/config --disable CONFIG_AGP_AMD64
        scripts/config --disable CONFIG_MICROCODE_AMD
        MARCH="MIVYBRIDGE"
        ;;
    ICELAKE-CLIENT) MARCH="ICELAKE" ;;
    esac

    # Add "M" prefix to MARCH variable
    MARCH2=M${MARCH}

    # show whiptail screen for the found CPU and ask if it is correct
    whiptail --title "CPU Architecture" --yesno "Detected CPU (MARCH) : ${MARCH2}\nIs this correct?" 10 60
    if [ $? -eq 1 ]; then
        # if not correct, ask for the CPU type
        MARCH2=$(whiptail --title "CPU Architecture" --inputbox "Enter CPU type (MARCH):" 10 60 "$MARCH2" 3>&1 1>&2 2>&3)
    fi

    # Display detected CPU and apply optimization
    echo "----------------------------------"
    echo "| APPLYING AUTO-CPU-OPTIMIZATION |"
    echo "----------------------------------"
    echo "[*] DETECTED CPU (MARCH) : ${MARCH2}"

    # define _march as MARCH2
    _march=$MARCH2
}

export NEWT_COLORS='
    root=white,blue
    border=black,lightgray
    window=black,lightgray
    shadow=black,gray
    title=black,lightgray
    button=black,cyan
    actbutton=white,blue
    checkbox=black,lightgray
    actcheckbox=black,cyan
    entry=black,lightgray
    label=black,lightgray
    listbox=black,lightgray
    actlistbox=black,cyan
    textbox=black,lightgray
    acttextbox=black,cyan
    helpline=white,blue
    roottext=black,lightgray
'

# Function to configure CPU scheduler
configure_cpusched() {
    local selection

    # Show radiolist and capture user selection
    selection=$(whiptail --title "CPU Scheduler Configuration" --radiolist \
        "Choose CPU Scheduler (best for games: BORE / CachyOS)" 20 74 8 \
        "bore"    "BORE scheduler - best for games" $([ "$_cpusched_selection" = "bore" ] && echo "ON" || echo "OFF") \
        "cachyos" "CachyOS preset - BORE-based, best for games" $([ "$_cpusched_selection" = "cachyos" ] && echo "ON" || echo "OFF") \
        "eevdf"   "Mainline EEVDF - balanced default" $([ "$_cpusched_selection" = "eevdf" ] && echo "ON" || echo "OFF") \
        "bmq"     "BMQ - low-latency alt scheduler" $([ "$_cpusched_selection" = "bmq" ] && echo "ON" || echo "OFF") \
        "rt"      "RT - real-time, not ideal for games" $([ "$_cpusched_selection" = "rt" ] && echo "ON" || echo "OFF") \
        "rt-bore" "RT + BORE - niche low-latency setup" $([ "$_cpusched_selection" = "rt-bore" ] && echo "ON" || echo "OFF") \
        "hardened" "Hardened BORE preset - security-focused" $([ "$_cpusched_selection" = "hardened" ] && echo "ON" || echo "OFF") \
        "none"    "Keep kernel defaults" $([ "$_cpusched_selection" = "none" ] && echo "ON" || echo "OFF") \
        3>&1 1>&2 2>&3)

    if [ $? -ne 0 ]; then
        return
    fi

    case "$selection" in
    bore | cachyos | eevdf | bmq | rt | rt-bore | hardened | none)
        _cpusched_selection="$selection"
        ;;
    esac
}

# Function to configure LLVM LTO
configure_llvm_lto() {
    local selection

    whiptail --title "LLVM LTO" --msgbox \
        "LLVM LTO optimizes code across kernel files to improve performance.\n\nThin is the best balanced choice for games. Full may need about 2 GB of RAM per core during build and is slower to compile.\n\nRecommended for gaming: Thin." \
        12 72

    selection=$(whiptail --title "LLVM LTO Configuration" --radiolist \
        "Choose LLVM LTO (use space to select):" 16 68 4 \
        "thin" "Enable LLVM LTO Thin" $([ "$_llvm_lto_selection" = "thin" ] && echo "ON" || echo "OFF") \
        "thin-dist" "Enable LLVM LTO Thin Dist" $([ "$_llvm_lto_selection" = "thin-dist" ] && echo "ON" || echo "OFF") \
        "full" "Enable LLVM LTO Full" $([ "$_llvm_lto_selection" = "full" ] && echo "ON" || echo "OFF") \
        "none" "Do not configure LLVM LTO" $([ "$_llvm_lto_selection" = "none" ] && echo "ON" || echo "OFF") \
        3>&1 1>&2 2>&3)

    if [ $? -ne 0 ]; then
        return
    fi

    case "$selection" in
    thin | thin-dist | full | none)
        _llvm_lto_selection="$selection"
        ;;
    esac
}

# Function to configure tick rate for 100|250|300|500|600|750|1000
configure_tick_rate() {
    local selection

    whiptail --title "Tick Rate" --msgbox \
        "Tick Rate (HZ) controls scheduler timer frequency.\n\nFor gaming, 1000 Hz gives the best responsiveness.\nCost: higher CPU overhead and power usage.\n\n500 Hz is a balanced alternative." \
        11 72

    selection=$(whiptail --title "Tick Rate Configuration" --radiolist \
        "Choose Tick Rate (use space to select):" 17 62 7 \
        "100" "100 Hz" $([ "$_tick_rate" = "100" ] && echo "ON" || echo "OFF") \
        "250" "250 Hz" $([ "$_tick_rate" = "250" ] && echo "ON" || echo "OFF") \
        "300" "300 Hz" $([ "$_tick_rate" = "300" ] && echo "ON" || echo "OFF") \
        "500" "500 Hz" $([ "$_tick_rate" = "500" ] && echo "ON" || echo "OFF") \
        "600" "600 Hz" $([ "$_tick_rate" = "600" ] && echo "ON" || echo "OFF") \
        "750" "750 Hz" $([ "$_tick_rate" = "750" ] && echo "ON" || echo "OFF") \
        "1000" "1000 Hz" $([ "$_tick_rate" = "1000" ] && echo "ON" || echo "OFF") \
        3>&1 1>&2 2>&3)

    if [ $? -ne 0 ]; then
        return
    fi

    case "$selection" in
    100 | 250 | 300 | 500 | 600 | 750 | 1000)
        _tick_rate="$selection"
        ;;
    esac

}

# Function to configure NR_CPUS
configure_nr_cpus() {
    local selection

    whiptail --title "NR_CPUS" --msgbox \
        "NR_CPUS is the maximum CPU/thread capacity compiled into the kernel.\n\nCurrent suggestion is ${_nr_cpus} (2x detected threads: ${_detected_threads}).\n\nLower values reduce memory overhead. Higher values add flexibility but increase kernel overhead.\n\nFor gaming, keep the suggested value unless you need a specific custom limit." \
        14 78

    selection=$(whiptail --title "NR_CPUS Configuration" --inputbox "Enter NR_CPUS value (integer >= 1):" 10 60 "$_nr_cpus" 3>&1 1>&2 2>&3)

    if [ $? -ne 0 ]; then
        return
    fi

    if [[ "$selection" =~ ^[0-9]+$ ]] && [ "$selection" -ge 1 ]; then
        _nr_cpus="$selection"
    else
        whiptail --title "Invalid NR_CPUS" --msgbox "Invalid value. Keeping current NR_CPUS: $_nr_cpus" 8 64
    fi
}

# Function to configure Hugepages
configure_hugepages() {
    local selection

    whiptail --title "Hugepages Note" --msgbox \
        "Hugepages guidance: madvise is the safest balanced choice for gaming and general use. always is for specific workloads. none leaves the kernel default untouched." \
        9 78

    if [ $? -ne 0 ]; then
        return
    fi

    selection=$(whiptail --title "Hugepages Configuration" --radiolist \
        "Choose Hugepages (use space to select):" 17 66 3 \
        "always" "Always use hugepages" $([ "$_hugepage" = "always" ] && echo "ON" || echo "OFF") \
        "madvise" "Use hugepages with madvise" $([ "$_hugepage" = "madvise" ] && echo "ON" || echo "OFF") \
        "none" "Do not configure Hugepages" $([ "$_hugepage" = "none" ] && echo "ON" || echo "OFF") \
        3>&1 1>&2 2>&3)

    if [ $? -ne 0 ]; then
        return
    fi

    case "$selection" in
    always | madvise | none)
        _hugepage="$selection"
        ;;
    esac
}

# Function to configure tick type
configure_tick_type() {
    local selection

    whiptail --title "Tick Type" --msgbox \
        "Tick Type controls when scheduler ticks run.\n\nnohz_full gives the lowest-latency behavior, but may require tuning for consistency.\nTuning is done after kernel install: boot params once (nohz_full/rcu_nocbs/isolcpus/irqaffinity) and runtime affinity each boot or via automation.\n\nnohz_idle is a balanced choice. periodic is the most conservative option.\n\nRecommended for gaming: nohz_full (or nohz_idle for better consistency)." \
        16 78

    selection=$(whiptail --title "Tick Type Configuration" --radiolist \
        "Choose Tick Type (use space to select):" 15 60 3 \
        "periodic" "Periodic tick" $([ "$_tick_type" = "periodic" ] && echo "ON" || echo "OFF") \
        "nohz_full" "Full dynticks" $([ "$_tick_type" = "nohz_full" ] && echo "ON" || echo "OFF") \
        "nohz_idle" "Idle dynticks" $([ "$_tick_type" = "nohz_idle" ] && echo "ON" || echo "OFF") \
        3>&1 1>&2 2>&3)

    if [ $? -ne 0 ]; then
        return
    fi

    case "$selection" in
    periodic | nohz_full | nohz_idle)
        _tick_type="$selection"
        ;;
    esac
}

configure_preempt_type() {
    local selection

    selection=$(whiptail --title "Preempt Type Configuration" --radiolist \
        "Choose Preempt Type (use space to select):" 16 62 4 \
        "voluntary" "Voluntary Preemption" $([ "$_preempt" = "voluntary" ] && echo "ON" || echo "OFF") \
        "preempt" "Preemptible Kernel" $([ "$_preempt" = "preempt" ] && echo "ON" || echo "OFF") \
        "preempt_dynamic" "Dynamic Preemption" $([ "$_preempt" = "preempt_dynamic" ] && echo "ON" || echo "OFF") \
        "none" "Do not configure Preempt Type" $([ "$_preempt" = "none" ] && echo "ON" || echo "OFF") \
        3>&1 1>&2 2>&3)

    if [ $? -ne 0 ]; then
        return
    fi

    case "$selection" in
    voluntary | preempt | preempt_dynamic | none)
        _preempt="$selection"
        ;;
    esac
}

configure_gaming_profile() {
    local page=1
    local selection

    while :; do
        if [ "$page" -eq 1 ]; then
            selection=$(whiptail --title "Gaming Profile" --menu \
                "Compilation summary (read-only) - page 1 of 3:" 20 78 6 \
                "1" "CPU Scheduler [$_cpusched_selection]" \
                "2" "LLVM LTO [$_llvm_lto_selection]" \
                "3" "Tick Rate [$_tick_rate Hz]" \
                "4" "Tick Type [$_tick_type]" \
                "n" "Next page" \
                "b" "Back" \
                3>&1 1>&2 2>&3)

            if [ $? -ne 0 ]; then
                return
            fi

            case "$selection" in
            n) page=2 ;;
            b) return ;;
            esac
        elif [ "$page" -eq 2 ]; then
            selection=$(whiptail --title "Gaming Profile" --menu \
                "Compilation summary (read-only) - page 2 of 3:" 20 78 6 \
                "5" "Preempt Type [$_preempt]" \
                "6" "Hugepages [$_hugepage]" \
                "7" "LRU [standard]" \
                "8" "NR_CPUS [$_nr_cpus]" \
                "n" "Next page" \
                "p" "Previous page" \
                3>&1 1>&2 2>&3)

            if [ $? -ne 0 ]; then
                return
            fi

            case "$selection" in
            n) page=3 ;;
            p) page=1 ;;
            esac
        else
            selection=$(whiptail --title "Gaming Profile" --menu \
                "Compilation summary (read-only) - page 3 of 3:" 20 78 6 \
                "9" "Performance Governor [$_performance_governor]" \
                "10" "ZFS [$_zfs]" \
                "11" "Set these in: System Optimizations" \
                "p" "Previous page" \
                "b" "Back" \
                3>&1 1>&2 2>&3)

            if [ $? -ne 0 ]; then
                return
            fi

            case "$selection" in
            p) page=2 ;;
            b) return ;;
            esac
        fi
    done
}

configure_system_optimizations() {
    # Initialize status of each optimization
    local o3_status=$([ "$_o3_optimization" = "yes" ] && echo "ON" || echo "OFF")
    local os_status=$([ "$_os_optimization" = "yes" ] && echo "ON" || echo "OFF")
    local performance_status=$([ "$_performance_governor" = "yes" ] && echo "ON" || echo "OFF")
    local bbr3_status=$([ "$_bbr3" = "yes" ] && echo "ON" || echo "OFF")
    local zfs_status=$([ "$_zfs" = "yes" ] && echo "ON" || echo "OFF")

    # Display checklist
    local selection
    selection=$(whiptail --title "System Optimizations Configuration" --checklist \
        "Select optimizations to enable:" 20 78 6 \
        "O3 Optimization" "" $o3_status \
        "OS Optimization" "" $os_status \
        "Performance Governor" "" $performance_status \
        "TCP BBR3" "" $bbr3_status \
        "ZFS" "" $zfs_status \
        3>&1 1>&2 2>&3)

    if [ $? -ne 0 ]; then
        return
    fi

    # Update configurations based on the selection
    if [[ "$selection" == *"O3 Optimization"* ]]; then
        _o3_optimization="yes"
        _os_optimization="no" # Disable OS Optimization if O3 Optimization is selected
    else
        _o3_optimization="no"
    fi

    if [[ "$selection" == *"OS Optimization"* ]]; then
        _os_optimization="yes"
        _o3_optimization="no" # Disable O3 Optimization if OS Optimization is selected
    else
        _os_optimization="no"
    fi

    [[ "$selection" == *"Performance Governor"* ]] && _performance_governor="yes" || _performance_governor="no"
    [[ "$selection" == *"TCP BBR3"* ]] && _bbr3="yes" || _bbr3="no"
    [[ "$selection" == *"ZFS"* ]] && _zfs="yes" || _zfs="no"
}

choose_kernel_option() {
    # 1. Busca específica: 7.x e toda a série 6.19
    local v7_list
    local v6_19_list
    
    whiptail --title "Sincronizando" --infobox "Buscando versões nos diretórios oficiais..." 8 50

    # Pega os 7.x disponíveis
    v7_list=$(curl -s https://cdn.kernel.org/pub/linux/kernel/v7.x/ | grep -oP 'linux-7\.[0-9]+(\.[0-9]+)?\.tar\.xz' | uniq | sort -rV)
    if [ -z "$v7_list" ]; then
        v7_list=$(curl -s https://cdn.kernel.org/pub/linux/kernel/v6.x/ | grep -oP 'linux-7\.[0-9]+(\.[0-9]+)?\.tar\.xz' | uniq | sort -rV)
    fi
    # Pega todos da série 6.19.*
    v6_19_list=$(curl -s https://cdn.kernel.org/pub/linux/kernel/v6.x/ | grep -oP 'linux-6\.19\.[0-9]+(\.[0-9]+)?\.tar\.xz' | uniq | sort -rV)

    # 2. Prepara o menu para o whiptail
    local menu_options=()
    
    # Adiciona a Série 7
    for name in $v7_list; do
        local ver=$(echo "$name" | sed 's/linux-//;s/.tar.xz//')
        menu_options+=("$ver" "[Série 7] - Mainline/Stable")
    done

    # Adiciona a Série 6.19
    for name in $v6_19_list; do
        local ver=$(echo "$name" | sed 's/linux-//;s/.tar.xz//')
        local label="[Série 6.19] - Recomendado"
        [[ "$ver" == "6.19.12" ]] && label="[Série 6.19.12] - BUILD SEGURA (PATRIES OK)"
        menu_options+=("$ver" "$label")
    done

    # 3. Exibe o menu de seleção
    local choice
    choice=$(whiptail --title "Seletor de Kernel Especializado" --menu "Escolha a versão exata para o seu i9:" 22 78 12 "${menu_options[@]}" 3>&1 1>&2 2>&3)

    # 4. Atualiza as variáveis globais com a URL correta
    if [ $? -eq 0 ]; then
        _kv_name="$choice"
        if [[ "$choice" == 7.* ]]; then
            _kv_url="https://cdn.kernel.org/pub/linux/kernel/v7.x/linux-$choice.tar.xz"
        else
            _kv_url="https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-$choice.tar.xz"
        fi
        whiptail --title "Configuração Aplicada" --msgbox "Versão definida: $_kv_name\nO script usará os patches da série ${_kv_name%.*}" 10 70
    fi
}

debing() {
    #!/bin/bash
    # Description: Script to compile a custom Linux kernel and package it into a .deb file for CachyOS
    # Maintainer: Laio O. Seman <laio@iee.org>

    KERNEL_VERSION=$(make kernelversion)
    ARCH=$(dpkg --print-architecture)

    # Kernel package variables
    KERNEL_PKG_NAME=custom-kernel-${KERNEL_VERSION}
    KERNEL_PKG_VERSION=${KERNEL_VERSION}-1
    KERNEL_PKG_DIR=${KERNEL_PKG_NAME}-${KERNEL_PKG_VERSION}

    # Headers package variables
    HEADERS_PKG_NAME=custom-kernel-headers-${KERNEL_VERSION}
    HEADERS_PKG_VERSION=${KERNEL_VERSION}-1
    HEADERS_PKG_DIR=${HEADERS_PKG_NAME}-${HEADERS_PKG_VERSION}

    # Function to create kernel package
    package_kernel() {
        # Create directory structure for kernel package
        mkdir -p ${KERNEL_PKG_DIR}/DEBIAN
        mkdir -p ${KERNEL_PKG_DIR}/boot
        mkdir -p ${KERNEL_PKG_DIR}/lib/modules/${KERNEL_VERSION}
        mkdir -p ${KERNEL_PKG_DIR}/usr/share/doc/${KERNEL_PKG_NAME}

        # Create control file for kernel package
        cat >${KERNEL_PKG_DIR}/DEBIAN/control <<EOF
Package: ${KERNEL_PKG_NAME}
Version: ${KERNEL_PKG_VERSION}
Section: kernel
Priority: optional
Architecture: ${ARCH}
Maintainer: CachyOs
Description: Custom compiled Linux Kernel
 Custom compiled Linux Kernel ${KERNEL_VERSION}
EOF

        # Copy the compiled kernel and modules
        cp arch/x86/boot/bzImage ${KERNEL_PKG_DIR}/boot/vmlinuz-${KERNEL_VERSION}
        cp -a /tmp/kernel-modules/lib/modules/${KERNEL_VERSION}/* ${KERNEL_PKG_DIR}/lib/modules/${KERNEL_VERSION}/
        cp System.map ${KERNEL_PKG_DIR}/boot/System.map-${KERNEL_VERSION}
        cp .config ${KERNEL_PKG_DIR}/boot/config-${KERNEL_VERSION}

        # Package the kernel
        fakeroot dpkg-deb --build ${KERNEL_PKG_DIR}

        # Clean up kernel package directory
        rm -rf ${KERNEL_PKG_DIR}
    }

    # Function to create headers package
    package_headers() {
        # Create directory structure for headers package
        mkdir -p ${HEADERS_PKG_DIR}/DEBIAN
        mkdir -p ${HEADERS_PKG_DIR}/usr/src/linux-headers-${KERNEL_VERSION}

        # Create control file for headers package
        cat >${HEADERS_PKG_DIR}/DEBIAN/control <<EOF
Package: ${HEADERS_PKG_NAME}
Version: ${HEADERS_PKG_VERSION}
Section: kernel
Priority: optional
Architecture: ${ARCH}
Maintainer: CachyOs
Description: Headers for custom compiled Linux Kernel ${KERNEL_VERSION}
EOF

        # Copy the kernel headers
        make headers_install INSTALL_HDR_PATH=${HEADERS_PKG_DIR}/usr/src/linux-headers-${KERNEL_VERSION}

        # Package the headers
        fakeroot dpkg-deb --build ${HEADERS_PKG_DIR}

        # Clean up headers package directory
        rm -rf ${HEADERS_PKG_DIR}
    }

    package_zfs() {

        ZFS_PKG_DIR=zfs-${KERNEL_VERSION}

        # Create directory structure for ZFS package
        mkdir -p ${ZFS_PKG_DIR}/DEBIAN

        # Create control file for ZFS package
        cat >zfs-${KERNEL_VERSION}/DEBIAN/control <<EOF
Package: zfs-${KERNEL_VERSION}
Version: ${KERNEL_PKG_VERSION}
Section: kernel
Priority: optional
Architecture: ${ARCH}
Maintainer: CachyOs
Description: ZFS for custom compiled Linux Kernel ${KERNEL_VERSION}
EOF

        # Copy the ZFS modules
        install -m644 module/*.ko "${ZFS_PKG_DIR}/lib/modules/${KERNEL_VERSION}/extra"
        find "$ZFS_PKG_DIR" -name '*.ko' -exec zstd --rm -10 {} +

        # Package the ZFS modules
        fakeroot dpkg-deb --build ${ZFS_PKG_DIR}

        # Clean up ZFS package directory
        rm -rf ${ZFS_PKG_DIR}
    }


    # Compile the kernel and modules
    make -j$(nproc)
    mkdir -p /tmp/kernel-modules
    make modules_install INSTALL_MOD_PATH=/tmp/kernel-modules

    if [ "$_zfs" == "yes" ]; then
        LINUX_DIR=$(pwd)
        git clone https://github.com/openzfs/zfs --depth 1
        cd zfs

        ./autogen.sh
        ./configure --prefix=/usr --sysconfdir=/etc --sbindir=/usr/bin \
            --libdir=/usr/lib --datadir=/usr/share --includedir=/usr/include \
            --with-udevdir=/lib/udev --libexecdir=/usr/lib/zfs --with-config=kernel \
            --with-linux="$LINUX_DIR"
        make -j$(nproc)
        cd "$LINUX_DIR"
    fi

    # Package the kernel
    package_kernel

    # Package the headers
    package_headers
    if [ "$_zfs" == "yes" ]; then
        package_zfs
    fi
    
}


do_things() {
    _major=$(echo $_kv_name | grep -oP '^\K[^\.]+')
    _mid=$(echo $_kv_name | grep -oP '^\d+\.\K[^\.]+')

    # 1. Download e Extração Limpa
    rm -rf "linux-$_kv_name" # Garante que não haja pasta antiga
    download_file "$_kv_url" "linux.tar.xz"
    tar -xf linux.tar.xz || die "Failed extracting linux.tar.xz"
    [ -d "linux-$_kv_name" ] || die "Extracted kernel directory not found: linux-$_kv_name"
    
    cd "linux-$_kv_name" || { echo "Erro ao entrar na pasta"; exit 1; }

    # 2. Configuração Base
    download_file "https://raw.githubusercontent.com/CachyOS/linux-cachyos/master/linux-cachyos/config" ".config"

    local _patchsource="https://raw.githubusercontent.com/cachyos/kernel-patches/master/${_major}.${_mid}"
    declare -a patches=()

    # 3. Lógica de Patches Adaptada (6.18+)
    scripts/config -e CACHYOS
    # Só tenta baixar o base se for versão antiga (< 6.18)
    if [ "$_major" -lt 6 ] || ([ "$_major" -eq 6 ] && [ "$_mid" -lt 18 ]); then
        patches+=("${_patchsource}/all/0001-cachyos-base-all.patch")
    fi

    ## Scheduler patches
    case "$_cpusched_selection" in
    bore | cachyos | hardened | rt-bore | rt)
        if [ "$_major" -lt 6 ] || ([ "$_major" -eq 6 ] && [ "$_mid" -lt 18 ]); then
            patches+=("${_patchsource}/sched-dev/0001-bore-cachy.patch|${_patchsource}/sched/0001-bore-cachy.patch")
        else
            patches+=("${_patchsource}/sched-dev/0001-bore.patch|${_patchsource}/sched/0001-bore.patch")
        fi
        ;;
    bmq)
        if [ "$_major" -lt 6 ] || ([ "$_major" -eq 6 ] && [ "$_mid" -lt 18 ]); then
            patches+=("${_patchsource}/sched-dev/0001-prjc-cachy-lfbmq.patch|${_patchsource}/sched/0001-prjc-cachy.patch")
        else
            patches+=("${_patchsource}/sched-dev/0001-prjc-lfbmq.patch|${_patchsource}/sched/0001-prjc.patch")
        fi
        ;;
    esac

    case "$_cpusched_selection" in
    rt | rt-bore)
        patches+=("${_patchsource}/misc/0001-rt-i915.patch")
        ;;
    hardened)
        patches+=("${_patchsource}/misc/0001-hardened.patch")
        ;;
    esac

    # 4. Aplicação dos Patches
    _scheduler_patch_failed=0
    for i in "${patches[@]}"; do
        echo "Baixando e aplicando: $i"
        if [[ "$i" == *"|"* ]]; then
            IFS='|' read -r -a patch_candidates <<< "$i"
            if ! download_and_apply_patch_candidates "${patch_candidates[@]}"; then
                echo "AVISO: Falha ao aplicar patch do scheduler: ${patch_candidates[0]##*/} (kernel $_kv_name)"
                echo "O kernel será compilado com o scheduler padrão (eevdf). Tente selecionar outro kernel ou desabilitar patches customizados."
                _scheduler_patch_failed=1
            fi
        else
            download_and_apply_patch "$i"
        fi
    done

    # Se o patch do scheduler falhou, desabilita CONFIG_SCHED_BORE
    if [ $_scheduler_patch_failed -eq 1 ]; then
        scripts/config -d SCHED_BORE 2>/dev/null || true
    fi

    if find . -type f -name '*.rej' | grep -q .; then
        echo "AVISO: Encontrados arquivos .rej (rejeição de patches)"
        # Não falha, apenas avisa
    fi

    # 5. Otimização Nativa (MALDERLAKE)
    scripts/config -k --disable CONFIG_GENERIC_CPU
    scripts/config -k --enable "CONFIG_${_march}"

    # Tick Rate (HZ)
    scripts/config -d HZ_100 -d HZ_250 -d HZ_300 -d HZ_500 -d HZ_600 -d HZ_750 -d HZ_1000
    case "$_tick_rate" in
    100 | 250 | 300 | 500 | 600 | 750 | 1000)
        scripts/config -e "HZ_${_tick_rate}" --set-val HZ "$_tick_rate"
        ;;
    *)
        scripts/config -e HZ_500 --set-val HZ 500
        ;;
    esac

    # Tick Type
    scripts/config -d HZ_PERIODIC -d NO_HZ_IDLE -d NO_HZ_FULL -d NO_HZ_FULL_NODEF -d NO_HZ -d NO_HZ_COMMON -d CONTEXT_TRACKING
    case "$_tick_type" in
    periodic)
        scripts/config -e HZ_PERIODIC
        ;;
    nohz_idle)
        scripts/config -e NO_HZ_IDLE -e NO_HZ -e NO_HZ_COMMON
        ;;
    nohz_full)
        scripts/config -e NO_HZ_FULL_NODEF -e NO_HZ_FULL -e NO_HZ -e NO_HZ_COMMON -e CONTEXT_TRACKING
        ;;
    *)
        scripts/config -e NO_HZ_FULL_NODEF -e NO_HZ_FULL -e NO_HZ -e NO_HZ_COMMON -e CONTEXT_TRACKING
        ;;
    esac

    # NR_CPUS
    if [[ "$_nr_cpus" =~ ^[0-9]+$ ]] && [ "$_nr_cpus" -ge 1 ]; then
        scripts/config --set-val NR_CPUS "$_nr_cpus"
    else
        scripts/config --set-val NR_CPUS 320
    fi

    # Hugepages
    if [ "$_hugepage" != "none" ]; then
        scripts/config -d TRANSPARENT_HUGEPAGE -d TRANSPARENT_HUGEPAGE_ALWAYS -d TRANSPARENT_HUGEPAGE_MADVISE -d TRANSPARENT_HUGEPAGE_NEVER
        case "$_hugepage" in
        always)
            scripts/config -e TRANSPARENT_HUGEPAGE -e TRANSPARENT_HUGEPAGE_ALWAYS
            ;;
        madvise)
            scripts/config -e TRANSPARENT_HUGEPAGE -e TRANSPARENT_HUGEPAGE_MADVISE
            ;;
        *)
            scripts/config -e TRANSPARENT_HUGEPAGE -e TRANSPARENT_HUGEPAGE_MADVISE
            ;;
        esac
    fi

    # LRU fixed to standard for compilation
    scripts/config -d LRU_GEN -d LRU_GEN_ENABLED -d LRU_GEN_STATS
    scripts/config -e LRU_GEN -e LRU_GEN_ENABLED

    # Preempt Type
    if [ "$_preempt" != "none" ]; then
        scripts/config -d PREEMPT_NONE -d PREEMPT_VOLUNTARY -d PREEMPT -d PREEMPT_DYNAMIC
        case "$_preempt" in
        voluntary)
            scripts/config -e PREEMPT_VOLUNTARY
            ;;
        preempt)
            scripts/config -e PREEMPT
            ;;
        preempt_dynamic)
            scripts/config -e PREEMPT_DYNAMIC
            ;;
        *)
            scripts/config -e PREEMPT
            ;;
        esac
    fi

    # Flags de Scheduler
    scripts/config -d SCHED_BORE -d SCHED_ALT -d SCHED_BMQ -d PREEMPT_RT
    case "$_cpusched_selection" in
    bore | cachyos | hardened | rt-bore)
        scripts/config -e SCHED_BORE
        ;;
    bmq)
        scripts/config -e SCHED_ALT -e SCHED_BMQ
        ;;
    rt | rt-bore)
        scripts/config -d PREEMPT_NONE -d PREEMPT_VOLUNTARY -d PREEMPT -d PREEMPT_DYNAMIC
        scripts/config -e PREEMPT_RT
        ;;
    esac

    # Flags de Performance
    scripts/config -d CC_OPTIMIZE_FOR_PERFORMANCE -e CC_OPTIMIZE_FOR_PERFORMANCE_O3

    # Performance Governor
    scripts/config -d CPU_FREQ_DEFAULT_GOV_POWERSAVE -d CPU_FREQ_DEFAULT_GOV_SCHEDUTIL -d CPU_FREQ_DEFAULT_GOV_USERSPACE -d CPU_FREQ_DEFAULT_GOV_ONDEMAND -d CPU_FREQ_DEFAULT_GOV_CONSERVATIVE -d CPU_FREQ_DEFAULT_GOV_PERFORMANCE
    if [ "$_performance_governor" = "yes" ]; then
        scripts/config -e CPU_FREQ_GOV_PERFORMANCE -e CPU_FREQ_DEFAULT_GOV_PERFORMANCE
    else
        scripts/config -e CPU_FREQ_GOV_SCHEDUTIL -e CPU_FREQ_DEFAULT_GOV_SCHEDUTIL
    fi

    # TCP BBR3 / BBR
    scripts/config -e NET -e INET -e TCP_CONG_ADVANCED -e NET_SCH_FQ
    if [ "$_bbr3" = "yes" ]; then
        if grep -q "CONFIG_TCP_CONG_BBR3" .config; then
            scripts/config -e TCP_CONG_BBR3
            if grep -q "CONFIG_DEFAULT_BBR3" .config; then
                scripts/config -d DEFAULT_RENO -d DEFAULT_CUBIC -e DEFAULT_BBR3 --set-str DEFAULT_TCP_CONG bbr3
            else
                scripts/config -d DEFAULT_RENO -d DEFAULT_CUBIC -e DEFAULT_BBR --set-str DEFAULT_TCP_CONG bbr
            fi
        else
            scripts/config -e TCP_CONG_BBR -d DEFAULT_RENO -d DEFAULT_CUBIC -e DEFAULT_BBR --set-str DEFAULT_TCP_CONG bbr
        fi
    else
        scripts/config -d DEFAULT_RENO -d DEFAULT_BBR -d DEFAULT_BBR3 -e DEFAULT_CUBIC --set-str DEFAULT_TCP_CONG cubic
    fi

    echo "Configurações aplicadas para $_kv_name!"
    debing
}

format_menu_item() {
    local left_text="$1"
    local right_text="$2"
    local left_width=48
    printf "%-${left_width}s%s" "$left_text" "$right_text"
}

# check if any argument was passed

if [ -n "$1" ]; then
    case "$1" in
    --help | -h)
        echo "Usage: $0"
        echo "Compile a custom Linux kernel and package it into a .deb file for CachyOS"
        exit 0
        ;;
    --build | -b)
        debing
        exit 0
        ;;
    esac
fi

# 1. Definimos os valores padrão (6.19.12)
_kv_url="https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-6.19.12.tar.xz"
_kv_name="6.19.12"

# 2. Chamada das funções de inicialização
check_deps
whiptail --title "CachyOS Kernel Configuration" --msgbox "This is a beta version..." 8 78
whiptail --title "Secure Boot Warning" --yesno "This script will disable secure boot..." 8 78

init_script
set_default_nr_cpus


# Main menu
# Main menu
while :; do
    # Labels dinâmicas para mostrar o que está selecionado
    sched_label="[$_cpusched_selection]"
    march_label="[$_march]"
    kernel_label="[$_kv_name]"
    lto_label="[$_llvm_lto_selection]"
    tickrate_label="[$_tick_rate Hz]"
    nrcpus_label="[$_nr_cpus]"
    ticktype_label="[$_tick_type]"
    preempt_label="[$_preempt]"
    hugepage_label="[$_hugepage]"

    CHOICE=$(whiptail --title "Kernel Configuration Menu" --menu "Choose an option" 22 78 16 \
        "0" "$(format_menu_item "Choose Kernel Version" "$kernel_label")" \
        "1" "$(format_menu_item "Configure CPU Scheduler" "$sched_label")" \
        "2" "$(format_menu_item "Configure MARCH" "$march_label")" \
        "3" "$(format_menu_item "Configure LLVM LTO" "$lto_label")" \
        "4" "$(format_menu_item "Configure Tick Rate" "$tickrate_label")" \
        "5" "$(format_menu_item "Configure NR_CPUS" "$nrcpus_label")" \
        "6" "$(format_menu_item "Configure Tick Type" "$ticktype_label")" \
        "7" "$(format_menu_item "Configure Preempt Type" "$preempt_label")" \
        "8" "$(format_menu_item "Configure Hugepages" "$hugepage_label")" \
        "9" "$(format_menu_item "Gaming Profile" "")" \
        "10" "$(format_menu_item "System Optimizations" "")" \
        "11" "$(format_menu_item "COMPILE KERNEL" "")" \
        "12" "$(format_menu_item "Exit" "")" 3>&1 1>&2 2>&3)

    exitstatus=$?
    if [ "$exitstatus" -ne 0 ]; then
        break
    fi

    case $CHOICE in
    0) choose_kernel_option ;;
    1) configure_cpusched ;;
    2)
        whiptail --title "MARCH Warning" --yesno "Only change MARCH if CPU auto-detection failed.\n\nUsing an incorrect value may cause build errors or unstable behavior.\n\nDo you want to continue with manual override?" 12 78
        if [ $? -eq 0 ]; then
            MARCH2=$(whiptail --title "CPU Architecture" --inputbox "Confirm CPU type (MARCH):" 10 60 "$_march" 3>&1 1>&2 2>&3)
            [ $? -eq 0 ] && _march=$MARCH2
        fi
        ;;
    3) configure_llvm_lto ;;
    4) configure_tick_rate ;;
    5) configure_nr_cpus ;;
    6) configure_tick_type ;;
    7) configure_preempt_type ;;
    8) configure_hugepages ;;
    9) configure_gaming_profile ;;
    10) configure_system_optimizations ;;
    11) do_things ;;
    12 | q) break ;;
    *) echo "Invalid Option" ;;
    esac
done
