// Milestone M1: a themed, empty window. Scan screens arrive in M3 (phase 7).
// File names are always rendered as text; never use dangerouslySetInnerHTML.
import appIcon from "../src-tauri/icons/source/space-optimiser-app-icon.svg";

export default function App() {
  return (
    <div className="app">
      <header className="app-header">
        <img className="app-icon" src={appIcon} alt="" width={32} height={32} />
        <span className="product-name">Space Optimiser</span>
      </header>
      <main className="app-main">
        <section className="card" aria-labelledby="status-title">
          <h1 id="status-title" className="title-lg">
            Scanning is not available yet
          </h1>
          <p className="body ink-muted">
            This build only opens the window. It does not read or remove any files.
          </p>
        </section>
      </main>
    </div>
  );
}
