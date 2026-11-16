#pragma once

#include <QString>
#include <QUrl>
#include <QUrlQuery>

namespace vortia {

/// A reviewable GitHub issue draft. Long bodies stay on the clipboard instead of being
/// truncated in a URL; opening this URL does not create an issue.
inline QUrl issueDraftUrl(const QString& title, const QString& body) {
    QUrl url(QStringLiteral("https://github.com/getnodalia/vortia/issues/new"));
    QUrlQuery query;
    query.addQueryItem(QStringLiteral("title"), title);
    query.addQueryItem(QStringLiteral("body"), body);
    url.setQuery(query);
    if (url.toEncoded().size() > 7000) {
        QUrlQuery titleOnly;
        titleOnly.addQueryItem(QStringLiteral("title"), title);
        url.setQuery(titleOnly);
    }
    return url;
}

} // namespace vortia
