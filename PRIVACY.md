# Privacy (draft)

> This is a draft. It will be finalised before result sharing ships.

Dynamis Power works entirely on your computer. Nothing leaves your machine unless you choose to share your results.

## What stays on your computer

- **The session agent** (`dynamis-agent`) tells the Dynamis Power service five yes/no facts about your desktop session: whether media is playing, the screen is locked, an app asked the screen to stay on, a game is running, and whether you are idle. It does not read or send window titles, app names, website addresses or what you are playing. The facts go to the service on your own computer and are kept only in memory.
- **Auto mode** also reads whether a microphone is recording (to recognise calls), how busy the processor is, and network traffic totals. None of this is stored except the reason line of each change in the journal (for example "auto: On a call: keeping sound and video steady").
- **Learning** keeps its test results (setting, value, measured watts and lag) and a brightness-versus-power model on your computer. They contain nothing about what you were doing.
- **The quirk list download** (`dynamis-sync`) fetches a public file from GitHub once a day. It sends nothing about you; GitHub sees an ordinary download.

## If you share results

Sharing is off by default. Before anything is sent, the app shows you every field that will be shared.

**What is shared:** your laptop model and main components (CPU, graphics, storage type, Wi-Fi chip, screen), kernel and firmware versions, Linux distribution and desktop, and the results of each setting test (how much it saved or cost, how sure the measurement was, and any faults).

**What is never shared:** app names, what you use your computer for, when you use it, your location, network names, serial numbers, usernames, or anything else that identifies you.

**Anonymous install ID.** Each install creates a random ID that is not linked to you, your account or your hardware serial numbers. It exists only so you can delete your submissions later.

## Your controls

- **Delete my submissions:** one button in the app removes everything sent from your install.
- **Stop sharing:** turn sharing off at any time; nothing more is sent.

## Retention

Raw submissions are kept for a limited period, to be set before launch, and then only the combined per-model summaries remain. The summaries are public and contain no install IDs.
