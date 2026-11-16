# Glossary

| Term | Meaning |
| --- | --- |
| **Calibration** | Testing one setting alone, on and off in turns, to measure its effect on this machine |
| **cgroup** | Kernel feature that groups processes so their CPU, memory and I/O can be measured and limited. Desktop apps each get one |
| **Daemon** | A background system service. Here, `dynamisd` |
| **Dead-man timer** | A trial setting reverts automatically unless the system stays healthy long enough |
| **D-Bus** | The message system Linux desktops use for programs to talk to each other |
| **EPP** | Energy-performance preference: a hint telling the CPU how eagerly to raise its speed |
| **Experiment record** | The stored result of one test: setting, value, context, effect, confidence, verdict |
| **Fixture** | A simulated `/sys` or `/proc` tree used by tests instead of real hardware |
| **Journal** | The log of every change, written before the change is applied |
| **Lever / setting** | One adjustable thing, such as turbo boost or Wi-Fi power save. Each has a permanent ID |
| **Overlay** | An option that combines with any profile, such as Battery Care |
| **Plugin** | Code that owns one sensor or one setting |
| **Profile** | A goal with limits, such as "fewest watts while stalls stay under 5%" |
| **PSI** | Pressure stall information: the share of time tasks wait for CPU, memory or I/O. The app's main "is it lagging" measure |
| **Quirk** | A known-bad combination of hardware and setting value, blocked by the quirk list |
| **RAPL** | Running Average Power Limit: Intel and AMD counters that report CPU and GPU energy, and set power limits |
| **Risk tier** | Safe, Usually safe, Risky, Experimental or Advisory. Decides how a setting is handled |
| **Safe mode** | Daemon start with stock settings, after repeated bad boots or when forced |
| **sched_ext** | Kernel feature for loading alternative CPU schedulers |
| **Session agent** | A small per-user process that reports what the user is doing |
| **sysfs** | The `/sys` filesystem where the kernel exposes hardware settings |
| **Trial** | A setting under test, not yet proven |
| **Workload class** | What the laptop is doing: idle, browsing, video, call, compute, gaming and so on |
