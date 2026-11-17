/** Advanced's tabs, in their own small module so the app can read
 * `#advanced:<tab>` without loading the Advanced screen's code. */
export const TABS = ["settings", "learning", "profiles", "sensors", "safety", "system", "developer"] as const;
export type Tab = (typeof TABS)[number];
