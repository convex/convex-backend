import { Link } from "@ui/Link";
import {
  commandPaletteFeedback,
  useFeedbackFormOpen,
} from "elements/FeedbackForm";

export function NoResultsMessage({ onClose }: { onClose: () => void }) {
  const [, setFeedbackOpen] = useFeedbackFormOpen();
  return (
    <>
      No results found.
      <span className="text-content-tertiary">
        Didn’t find what you’re looking for?{" "}
        <Link
          href="#"
          onClick={(e) => {
            e.preventDefault();
            // Close the palette first: the feedback form lives outside it, and
            // this releases the Radix focus trap so the form can take focus.
            onClose();
            setFeedbackOpen(commandPaletteFeedback);
          }}
        >
          Send feedback
        </Link>
        .
      </span>
    </>
  );
}
