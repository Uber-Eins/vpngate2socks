import { zhCN as t } from "../i18n";
import { Button } from "../ui/Button";
import { ChevronLeftIcon, ChevronRightIcon } from "../ui/Icon";
import { formatNumber } from "../utils/format";

export function Pagination({ page, totalPages, totalItems, onPage }: {
  page: number;
  totalPages: number;
  totalItems: number;
  onPage: (page: number) => void;
}) {
  return (
    <nav className="pagination" aria-label={t.navNodes}>
      <span>
        <strong className="num">{formatNumber(totalItems)}</strong> {t.resultsCount}
      </span>
      <div className="pagination__controls">
        <span className="pagination__position">
          {t.pagePosition.replace("{page}", String(page)).replace("{total}", String(totalPages))}
        </span>
        <Button
          size="sm"
          iconOnly
          aria-label={t.previousPage}
          disabled={page <= 1}
          onClick={() => onPage(page - 1)}
        >
          <ChevronLeftIcon size={14} />
        </Button>
        <Button
          size="sm"
          iconOnly
          aria-label={t.nextPage}
          disabled={page >= totalPages}
          onClick={() => onPage(page + 1)}
        >
          <ChevronRightIcon size={14} />
        </Button>
      </div>
    </nav>
  );
}
