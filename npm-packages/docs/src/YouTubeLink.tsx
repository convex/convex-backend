import React from "react";

function PlayIcon(props: React.SVGProps<SVGSVGElement>) {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor" {...props}>
      <path d="M8 5v14l11-7z" />
    </svg>
  );
}

type YouTubeItem = {
  src: string;
  label: string;
};

export function YouTubeList({ items }: { items: YouTubeItem[] }) {
  return (
    <div className="video-cards">
      {items.map((item) => {
        // Extract video ID from src URL
        const videoId = item.src.split("/").pop()?.split("?")[0];
        return (
          <a
            key={item.label}
            href={`https://www.youtube.com/watch?v=${videoId}`}
            target="_blank"
            rel="noopener noreferrer"
            className="video-card"
          >
            <PlayIcon className="play-icon" />
            <h2>{item.label}</h2>
          </a>
        );
      })}
    </div>
  );
}
