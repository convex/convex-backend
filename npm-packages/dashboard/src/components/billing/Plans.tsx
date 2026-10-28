import { useListPlans } from "api/billing";
import {
  OrbSubscriptionResponse,
  PlanResponse,
  TeamResponse,
} from "generatedApi";
import { OrbSelfServePlan } from "./planCards/OrbSelfServePlan";
import { FreePlan } from "./planCards/FreePlan";
import { BusinessPlan } from "./planCards/BusinessPlan";

const placeholderPlans: PlanResponse[] = [
  {
    id: "placeholder-starter",
    planType: "CONVEX_STARTER_PLUS",
    name: "Starter",
    description: "",
    status: "active",
    seatPrice: null,
  },
  {
    id: "placeholder-professional",
    planType: "CONVEX_PROFESSIONAL",
    name: "Professional",
    description: "",
    status: "active",
    seatPrice: null,
  },
];

export function Plans({
  team,
  subscription,
}: {
  team: TeamResponse;
  subscription?: OrbSubscriptionResponse;
}) {
  const orbPlans = useListPlans(team.id);
  const isLoading = orbPlans.plans === undefined;
  const plans = orbPlans.plans ?? placeholderPlans;

  return (
    <div className="scrollbar flex gap-3 overflow-x-auto pb-2">
      <FreePlan subscription={subscription} team={team} isLoading={isLoading} />
      {plans.map((plan, idx) => (
        <OrbSelfServePlan
          key={idx}
          orbSub={subscription}
          plan={plan}
          team={team}
          isLoading={isLoading}
        />
      ))}
      <BusinessPlan subscription={subscription} isLoading={isLoading} />
    </div>
  );
}
