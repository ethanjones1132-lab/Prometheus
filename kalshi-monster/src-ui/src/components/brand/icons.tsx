import type { ReactNode } from 'react';

/**
 * Line-icon set for the primary navigation — thin gold strokes in the
 * spirit of the identity's constellation diagrams.
 */

function base(children: ReactNode) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {children}
    </svg>
  );
}

export function CommandDeskIcon() {
  return base(
    <>
      <circle cx="12" cy="12" r="8.5" />
      <circle cx="12" cy="12" r="4.5" />
      <circle cx="12" cy="12" r="1" fill="currentColor" stroke="none" />
      <path d="M12 3.5v3M12 17.5v3M3.5 12h3M17.5 12h3" />
    </>,
  );
}

export function WorldMarketsIcon() {
  return base(
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M3.5 12h17M12 3.5c2.6 2.3 3.9 5.2 3.9 8.5s-1.3 6.2-3.9 8.5c-2.6-2.3-3.9-5.2-3.9-8.5S9.4 5.8 12 3.5Z" />
    </>,
  );
}

export function AnalystEyeIcon() {
  return base(
    <>
      <path d="M2.5 12S6 5.8 12 5.8 21.5 12 21.5 12 18 18.2 12 18.2 2.5 12 2.5 12Z" />
      <circle cx="12" cy="12" r="2.8" />
      <circle cx="12" cy="12" r="0.6" fill="currentColor" stroke="none" />
    </>,
  );
}

export function PortfolioIcon() {
  return base(
    <>
      <path d="M4 7.5h16v11H4z" />
      <path d="M9 7.5V6a2 2 0 0 1 2-2h2a2 2 0 0 1 2 2v1.5" />
      <path d="M4 12.5h16" />
    </>,
  );
}

export function CalibrationIcon() {
  return base(
    <>
      <path d="M4.5 18a8.5 8.5 0 1 1 15 0" />
      <path d="M12 13.5 15.5 8" />
      <circle cx="12" cy="14" r="1.4" />
      <path d="M5.5 14h.01M18.5 14h.01M12 6.5h.01" />
    </>,
  );
}

export function SettingsIcon() {
  return base(
    <>
      <circle cx="12" cy="12" r="2.6" />
      <path d="M12 3.8v2M12 18.2v2M3.8 12h2M18.2 12h2M6.2 6.2l1.4 1.4M16.4 16.4l1.4 1.4M17.8 6.2l-1.4 1.4M7.6 16.4l-1.4 1.4" />
    </>,
  );
}
