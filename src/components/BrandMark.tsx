interface BrandMarkProps {
  compact?: boolean;
}

export function BrandMark({ compact = false }: BrandMarkProps) {
  return (
    <div className="flex items-center gap-2.5 shrink-0" aria-label="uncord">
      {/* Concept C: Hardware Port symbol (exact geometry from brand/concepts/concept-c-symbol.svg) */}
      <svg
        xmlns="http://www.w3.org/2000/svg"
        viewBox="0 0 256 256"
        width={28}
        height={28}
        aria-hidden="true"
        className="shrink-0"
      >
        <path
          fill="var(--logo-plate)"
          fillRule="evenodd"
          d="M 72 24 H 98 V 80 A 18 18 0 0 0 62 80 V 128 A 66 66 0 0 0 194 128 V 25.05 A 48 48 0 0 1 232 72 V 184 A 48 48 0 0 1 184 232 H 72 A 48 48 0 0 1 24 184 V 72 A 48 48 0 0 1 72 24 Z"
        />
        <path
          fill="var(--accent-signal)"
          d="M 98 24 H 158 V 128 A 30 30 0 0 1 98 128 Z"
        />
      </svg>

      {!compact && (
        <svg
          xmlns="http://www.w3.org/2000/svg"
          viewBox="272 34 664 162"
          height={18}
          aria-hidden="true"
          className="shrink-0"
          style={{ width: "auto", color: "var(--text-primary)" }}
        >
          <g fill="currentColor">
            <path d="M 276 74 H 298 V 142 A 24 28 0 0 0 346 142 V 74 H 368 V 188 H 346 V 171 C 340 184 330 190 317 190 C 292 190 276 172 276 142 Z" />
            <path d="M 392 74 H 414 V 91 C 420 78 430 72 443 72 C 468 72 484 90 484 120 V 188 H 462 V 120 A 24 28 0 0 0 414 120 V 188 H 392 Z" />
            <path d="M 602.73 110 H 579.59 A 28 39 0 1 0 579.59 152 H 602.73 A 50 59 0 1 1 602.73 110 Z" />
            <path
              fillRule="evenodd"
              d="M 676 72 A 55 59 0 1 1 676 190 A 55 59 0 1 1 676 72 Z M 676 92 A 33 39 0 1 0 676 170 A 33 39 0 1 0 676 92 Z"
            />
            <path d="M 751 74 H 773 V 92 C 779 78 789 72 802 72 H 813 V 94 H 803 C 785 94 773 106 773 124 V 188 H 751 Z" />
            <path
              fillRule="evenodd"
              d="M 910 36 H 932 V 188 H 910 V 171 C 902 184 890 190 874 190 C 845 190 825 166 825 131 C 825 96 845 72 874 72 C 890 72 902 78 910 91 Z M 910 108 C 904 97 893 92 879 92 C 861.3 92 847 109.5 847 131 C 847 152.5 861.3 170 879 170 C 893 170 904 165 910 154 Z"
            />
          </g>
        </svg>
      )}
    </div>
  );
}
