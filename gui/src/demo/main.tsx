// The clickable demo: the real app on the stand-in backend, inside a pretend
// desktop with the panel icon. Built with `npm run build:demo`; never part
// of the installed app.
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "../styles.css";
import "./demo.css";
import { backend } from "../backend";
import { Demo } from "./Demo";

// A shared page cannot download files; say so instead of pretending.
backend.saveFile = async (name) => `nowhere: this is the demo (the real app saves ${name} to your Downloads folder)`;

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Demo />
  </StrictMode>,
);
