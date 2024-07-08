use core::arch::x86_64::{__cpuid, __rdtscp, _mm_lfence, _mm_pause, _rdtsc};
use core::ptr;
use core::time::Duration;

use crate::acpi::madt::{Madt, MadtEntry};
use crate::arch::x86_64::cpu::{read_msr, write_msr};
use crate::arch::x86_64::idt::Idt;
use crate::arch::x86_64::interrupts::apic_consts::{APIC_DISABLE, APIC_NMI, DESTINATION_FORMAT, EOI_REGISTER, LAPIC_VERSION, LOGICAL_DESTINATION, LVT_LINT0, LVT_LINT1, LVT_PERFORMANCE_MONITORING_COUNTERS, LVT_TIMER, TASK_PRIORITY_TPR, TIMER_DIVISOR, TIMER_CURRENT, TIMER_INIT_COUNT, SPURIOUS_INTERRUPT_VECTOR, APIC_SW_ENABLE};
use crate::arch::x86_64::interrupts::isa_handler::{load_dummy_handlers, set_isr, timer_handler};

const FEAT_EDX_APIC: u32 = 1 << 9;
const APIC_MSR: u32 = 0x1B;

pub struct Apic {
    base_phys_addr: usize,
    base_mapped_addr: Option<usize>,
    pub tps: u64,
    pub timer: u32,
    pub timer_div: u32,
    pub lvt_max: u8,
}


#[repr(u32)]
/// Masks bits 17 & 18 as per fig 11-8 from the intel SDM Vol 3 11.5
pub enum TimerMode {
    // makes the bits for mode 00, so is a no op
    Oneshot = 0x00,
    Periodic = 0x01 << 17,
    TscDeadline = 0x02 << 17,
}

impl Apic {
    pub fn new(madt: &Madt) -> Self {
        let addr = Self::get_apic_addr(madt);

        Apic {
            base_phys_addr: addr,
            base_mapped_addr: None,
            tps: 0,
            timer: 0,
            timer_div: 0,
            lvt_max: 0,
        }
    }

    pub fn is_present() -> bool {
        let cpuid = unsafe { __cpuid(1) };
        (cpuid.edx & FEAT_EDX_APIC) == FEAT_EDX_APIC
    }

    fn get_addr(&self) -> usize {
        self.base_mapped_addr.unwrap_or(self.base_phys_addr)
    }

    #[allow(unused)]
    pub fn get_apic_addr(madt: &Madt) -> usize {
        let mut addr = madt.local_apic_addr() as usize;
        let mut itr = madt.iter();
        for entry in itr {
            if let MadtEntry::LocalApicAddressOverride(addr_o) = entry {
                addr = addr_o.local_apic_address as usize;
            }
        }

        addr
    }

    pub fn enable_apic(enable: bool) {
        let mut msr = read_msr(APIC_MSR);

        if enable {
            msr.eax |= 1 << 11;
        } else {
            msr.eax ^= 1 << 11;
        }
        write_msr(APIC_MSR, msr);
    }

    pub fn is_apic_enabled() -> bool {
        let msr = read_msr(APIC_MSR);

        (msr.eax & 1 << 11) != 0
    }

    pub fn write_apic_reg(&self, offset: u32, value: u32) {
        let addr = (self.get_addr() + offset as usize) as *mut u32;
        unsafe { ptr::write_volatile(addr, value) }
    }

    pub fn read_apic_reg(&self, offset: u32) -> u32 {
        let addr = (self.get_addr() + offset as usize) as *const u32;
        unsafe { ptr::read_volatile(addr) }
    }

    pub fn init(&mut self) {
        // If the apic is not present according to cpuid
        if !Apic::is_present() {
            panic!("APIC is not present, and is required!")
        }

        let ver_reg = self.read_apic_reg(LAPIC_VERSION);
        let max_lvt = (0xffu32 << 16) & ver_reg;
        // this is a valid downcast, by moving the max_lvt 16 bits to the right, we get the max lvt
        // in the lower 8 bits of the u32 and can then transmute it to an u8.
        self.lvt_max = ((max_lvt >> 16) + 1) as u8;

        // initialize the APIC to known state
        let base = self.get_addr();
        if base != 0xFEE00000 {
            panic!("APIC base address is not 0xFEE00000, it is {:#X}", base);
        }
        // reset the apic to make sure it's in a known state
        Self::enable_apic(false);
        Self::enable_apic(true);

        self.write_apic_reg(DESTINATION_FORMAT, 0x0FFFFFFFF);
        let ldf = self.read_apic_reg(LOGICAL_DESTINATION) & 0x00FFFFFF;
        self.write_apic_reg(LOGICAL_DESTINATION, ldf);
        self.write_apic_reg(SPURIOUS_INTERRUPT_VECTOR, 0x27+APIC_SW_ENABLE);
        self.write_apic_reg(LVT_TIMER, APIC_DISABLE);
        self.write_apic_reg(LVT_PERFORMANCE_MONITORING_COUNTERS, APIC_NMI);
        self.write_apic_reg(LVT_LINT0, APIC_DISABLE);
        self.write_apic_reg(LVT_LINT1, APIC_DISABLE);
        self.write_apic_reg(TASK_PRIORITY_TPR, 0);

        self.tps = self.calculate_ticks_per_second();
        self.write_eoi();
    }

    pub fn enable(&mut self, idt: &mut Idt) {
        load_dummy_handlers(idt);
        // set the timer interrupt handler
        set_isr(idt, 0x20, timer_handler);
    }

    fn measure_tsc_duration(duration: Duration) -> u64 {
        unsafe {
            _mm_lfence(); // Serialize
            let start_tsc = __rdtscp(&mut 0);
            _mm_lfence(); // Serialize

            let start_time = _rdtsc();

            // Busy-wait loop for the specified duration
            let end_time = start_time + duration.as_nanos() as u64;
            while _rdtsc() < end_time {
                _mm_pause();
            }

            _mm_lfence(); // Serialize
            let end_tsc = __rdtscp(&mut 0);
            _mm_lfence(); // Serialize

            end_tsc - start_tsc
        }
    }

    fn calculate_bus_speed(ticks: u64, duration: Duration) -> u64 {
        ticks / duration.as_secs()
    }

    fn calculate_ticks_per_second(&self) -> u64 {
        let duration = Duration::from_secs(1);
        let ticks = Self::measure_tsc_duration(duration);
        Self::calculate_bus_speed(ticks, duration)
    }

    pub fn set_timer_counter(&self, frequency: u32) {
        let ticks_per_second = self.calculate_ticks_per_second();
        let counter_value = ticks_per_second / frequency as u64;
        self.write_apic_reg(TIMER_INIT_COUNT, counter_value as u32);
    }

    pub fn set_timer_divisor(&self, divisor: u8) {
        self.write_apic_reg(TIMER_DIVISOR, divisor as u32);
    }

    pub fn setup_timer(&self, mode: TimerMode, frequency: u32, divisor: u8) {
        self.set_timer_counter(frequency);
        self.set_timer_divisor(divisor);
        self.set_lvt_timer_register(mode, true, 0x20);
        self.set_timer_divisor(divisor);
    }

    pub fn set_lvt_timer_register(&self, mode: TimerMode, enabled: bool, vector: u8) {
        let mut value = vector as u32;
        value |= mode as u32;
        // if masking is desired by enabled being false
        // then or 1 << 16 with the mask
        if !enabled {
            value
                |= 1 << 16;
        }
        self.write_apic_reg(LVT_TIMER, value);
    }

    pub fn write_eoi(&self) {
        self.write_apic_reg(EOI_REGISTER, 1);
    }

    pub fn get_timer_current_count(&self) -> u32 {
        self.read_apic_reg(TIMER_CURRENT)
    }
}

// IO APIC Configuration
pub struct IoApic {
    base_phys_addr: usize,
    base_mapped_addr: Option<usize>,
    id: u8,
}

impl IoApic {
    pub fn new(base_phys_addr: usize, id: u8) -> Self {
        IoApic {
            base_phys_addr,
            base_mapped_addr: None,
            id,
        }
    }

    pub fn map_interrupt(&self, irq: u8, vector: u8) {
        // Map the IRQ to the interrupt vector in the IO APIC
    }
}

// Serial Port Interrupts

pub fn init_serial_interrupts() {
    unsafe {
        // Enable interrupts on the serial port (COM1)
        let port = 0x3F8;
        outb(port + 1, 0x01); // Enable interrupt on received data available
    }
}

unsafe fn outb(port: u16, value: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") value);
}

unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    core::arch::asm!("in al, dx", out("al") value, in("dx") port);
    value
}
