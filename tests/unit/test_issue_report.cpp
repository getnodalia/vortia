#include "vortia/ui/IssueReport.hpp"
#include <QUrlQuery>
#include <gtest/gtest.h>

TEST(IssueReport, PreservesTextAndUsesOnlyTheVortiaRepository) {
    const QString title = QString::fromUtf8("Error & pérdida");
    const QString body = QString::fromUtf8("Paso 1: a & b = 2\n¿Qué pasó?");
    const auto url = vortia::issueDraftUrl(title, body);
    EXPECT_EQ(url.host(), QStringLiteral("github.com"));
    EXPECT_EQ(url.path(), QStringLiteral("/getnodalia/vortia/issues/new"));
    const QUrlQuery query(url);
    EXPECT_EQ(query.queryItemValue(QStringLiteral("title"), QUrl::FullyDecoded), title);
    EXPECT_EQ(query.queryItemValue(QStringLiteral("body"), QUrl::FullyDecoded), body);
}

TEST(IssueReport, LongReportsStayOutOfTheUrl) {
    const auto url = vortia::issueDraftUrl(QStringLiteral("Problem"), QString(9000, QLatin1Char('x')));
    EXPECT_FALSE(QUrlQuery(url).hasQueryItem(QStringLiteral("body")));
    EXPECT_EQ(QUrlQuery(url).queryItemValue(QStringLiteral("title")), QStringLiteral("Problem"));
}
