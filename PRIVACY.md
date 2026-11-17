# Privacy (draft)

> This is a draft. It will be finalised before result sharing ships.

Dynamis Power works entirely on your computer. Nothing leaves your machine unless you choose to share your results.

## What stays on your computer

- **The session agent** (`dynamis-agent`) tells the Dynamis Power service five yes/no facts about your desktop session: whether media is playing, the screen is locked, an app asked the screen to stay on, a game is running, and whether you are idle. On KDE Plasma it also sends the process number of the window in front. It does not read or send window titles, website addresses or what you are playing. The facts go to the service on your own computer and are kept only in memory.
- **Apps.** The service reads which app each process belongs to (from the app's own system group, for example `app-firefox-1234.scope`) and how much processor time it used. It keeps a per-day total of processor time and estimated battery use per app, for the last week, on your computer only (`/var/lib/dynamis-power/apps.json`). Your choices for apps (never slow, always slow, a mode while open) are kept there too. *Forget app history* in the app deletes the totals, and switching counting off stops it. App names are never part of shared results.
- **Auto mode** also reads whether a microphone is recording (to recognise calls), how busy the processor is, and network traffic totals. None of this is stored except the reason line of each change in the journal (for example "auto: On a call: keeping sound and video steady").
- **Habits** (phase 7): typical power on battery for each hour of the week, and the times the charger was unplugged (the last 60). Used for the battery forecast and for charging that follows your routine. Kept on this computer only (`/var/lib/dynamis-power/habits.json`) and never shared.
- **Learning** keeps its test results (setting, value, measured watts and lag) and a brightness-versus-power model on your computer. They contain nothing about what you were doing.
- **The quirk list download** (`dynamis-sync`) fetches a public file from GitHub once a day. It sends nothing about you; GitHub sees an ordinary download.

## If you share results

Sharing is off by default. Before anything is sent, the app shows you every field that will be shared.

**What is shared:** your laptop model and main components (CPU, graphics, storage type, Wi-Fi chip, screen), kernel and firmware versions, Linux distribution and desktop, and the results of each setting test (how much it saved or cost, how sure the measurement was, and any faults).

**What is never shared:** app names, what you use your computer for, when you use it, your location, network names, serial numbers, usernames, or anything else that identifies you.

**You submit through GitHub, and your GitHub account is shown.** Sharing opens a form on GitHub in your browser; you sign in and press Submit yourself. Dynamis Power never sees your GitHub password or a token. Your submission is public, and GitHub shows the account that made it. The results inside contain nothing personal. Downloading other people's shared results needs no account.

**Anonymous install ID.** Each install creates a random ID that is not linked to you or your hardware serial numbers. It lets several submissions from one install count as one report.

## Your controls

- **Delete my submissions:** the app's button opens a page on GitHub; commenting `/delete` on your submission, or opening a "Delete my results" issue, removes every file you submitted and rebuilds the summaries without them.
- **Stop sharing:** turn sharing off at any time; nothing more is sent.

## Retention

Submissions are public files in the results repository until you delete them; how long they are kept will be set before launch. The per-model summaries are public and contain no install IDs or account names.
