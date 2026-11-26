import { Component, ReactNode } from "react";

/** Shows what went wrong instead of a blank window when a screen fails to render. */
export class ErrorBoundary extends Component<{ children: ReactNode }, { error?: Error }> {
  state: { error?: Error } = {};

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="pad">
        <div className="card problem">
          <b>This screen couldn't be shown.</b>
          <pre className="selectable mono" style={{ whiteSpace: "pre-wrap", marginTop: 8 }}>{String(this.state.error.stack ?? this.state.error)}</pre>
          <div className="actions"><button className="btn" onClick={() => this.setState({ error: undefined })}>Try Again</button></div>
        </div>
      </div>
    );
  }
}
