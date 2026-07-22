import { zhCN as t } from "../i18n";

export function Pagination(props: {
  page: number;
  totalPages: number;
  totalItems: number;
  onPage: (page: number) => void;
}) {
  return (
    <footer className="pagination" aria-label="节点分页">
      <span><strong>{props.totalItems.toLocaleString()}</strong> {t.results}</span>
      <div className="pagination__position">
        <span>{props.page} / {props.totalPages} {t.page}</span>
        <div>
          <button
            className="button button--quiet button--icon"
            type="button"
            aria-label={t.previous}
            disabled={props.page <= 1}
            onClick={() => props.onPage(props.page - 1)}
          >
            ←
          </button>
          <button
            className="button button--quiet button--icon"
            type="button"
            aria-label={t.next}
            disabled={props.page >= props.totalPages}
            onClick={() => props.onPage(props.page + 1)}
          >
            →
          </button>
        </div>
      </div>
    </footer>
  );
}
